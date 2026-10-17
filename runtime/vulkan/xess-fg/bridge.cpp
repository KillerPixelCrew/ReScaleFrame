// XeSS-FG 3.0.2 DX12 presentation bridge. The Intel binaries are loaded from
// this module's directory; no SDK binary is modified or embedded here.
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <vulkan/vulkan.h>
#include <cstdint>
#include <cstring>
#include <cstddef>
#include <array>
#include <new>
#include <string>
#include <cstdio>
#include <cstdlib>
#include "xess_fg/xefg_swapchain_d3d12.h"
#include "xell/xell_d3d12.h"

/// @brief Releases a local COM reference and clears its pointer without GPU waiting.
template<typename T> static void Release(T*& value) {
    if (value) { value->Release(); value = nullptr; }
}

// Opt-in present-phase timing (VULKANSTORY_XESS_FG_TIMING=1): mean and max milliseconds
// per phase, written to stderr every 120 presents. Diagnostics only.
namespace {
/// @brief CPU presentation-phase indices used only by opt-in XeSS diagnostics.
enum TimingPhase { kAllocatorWait, kRecord, kSubmit, kPresent, kRetire, kSleep, kPhaseCount };
const char* const kPhaseNames[kPhaseCount] = {
    "allocator_wait", "record", "submit", "dxgi_present", "retire", "xell_sleep" };
/// @brief Guards the diagnostic accumulators: the render thread records XeLL sleep while the present thread records present phases.
SRWLOCK timingLock = SRWLOCK_INIT;
/// @brief Holds timingLock exclusively for one scope.
struct TimingGuard {
    TimingGuard() { AcquireSRWLockExclusive(&timingLock); }
    ~TimingGuard() { ReleaseSRWLockExclusive(&timingLock); }
    TimingGuard(const TimingGuard&) = delete;
    TimingGuard& operator=(const TimingGuard&) = delete;
};
/// @brief Bounded opt-in CPU phase and calibrated GPU timing accumulators.
/// @details enabled and frequency are fixed at load. The accumulators are written from the render thread (XeLL sleep) and the present thread, so every access holds timingLock. Every 120 presents the formatted record is written and the accumulators reset.
struct PresentTiming {
    const bool enabled = std::getenv("VULKANSTORY_XESS_FG_TIMING") != nullptr;
    const double frequency = [] { LARGE_INTEGER f; QueryPerformanceFrequency(&f); return static_cast<double>(f.QuadPart); }();
    double sum[kPhaseCount]{};
    double max[kPhaseCount]{};
    uint32_t presents = 0;
    // DX12 queue work of one present (our copy list through the list submitted
    // after Present, so it includes the SDK's interpolation on this queue),
    // placed on the CPU clock through the queue's clock calibration.
    double gpuSum = 0, gpuMax = 0;
    double copiesSum = 0;
    double startAfterEnterSum = 0;
    double exitAfterGpuEndSum = 0;
    uint32_t gpuSamples = 0;
    double sequence[24]{};
    uint32_t sequenceCount = 0;
    int64_t Now() const { LARGE_INTEGER t; QueryPerformanceCounter(&t); return t.QuadPart; }
    void Add(TimingPhase phase, int64_t start, int64_t end) {
        double ms = (end - start) * 1000.0 / frequency;
        TimingGuard guard;
        sum[phase] += ms;
        if (ms > max[phase]) max[phase] = ms;
        if (phase == kPresent && sequenceCount < 24) sequence[sequenceCount++] = ms;
    }
    void EndPresent() {
        TimingGuard guard;
        if (++presents < 120) return;
        std::fprintf(stderr, "[xess-fg timing] presents=%u", presents);
        for (int i = 0; i < kPhaseCount; i++)
            std::fprintf(stderr, " %s_mean_ms=%.3f %s_max_ms=%.3f", kPhaseNames[i], sum[i] / presents,
                kPhaseNames[i], max[i]);
        if (gpuSamples)
            std::fprintf(stderr, " dx12_gpu_mean_ms=%.3f dx12_gpu_max_ms=%.3f gpu_start_after_present_enter_ms=%.3f"
                " present_exit_after_gpu_end_ms=%.3f dx12_our_copies_ms=%.3f", gpuSum / gpuSamples, gpuMax,
                startAfterEnterSum / gpuSamples, exitAfterGpuEndSum / gpuSamples, copiesSum / gpuSamples);
        std::fprintf(stderr, " sequence_ms=");
        for (uint32_t i = 0; i < sequenceCount; i++) std::fprintf(stderr, i ? ",%.1f" : "%.1f", sequence[i]);
        std::fputc('\n', stderr);
        std::fflush(stderr);
        ResetAccumulators();
    }
    /// @brief Clears every accumulator; the caller holds timingLock.
    void ResetAccumulators() {
        for (int i = 0; i < kPhaseCount; i++) { sum[i] = 0; max[i] = 0; }
        presents = 0;
        gpuSum = gpuMax = 0;
        copiesSum = 0;
        startAfterEnterSum = 0;
        exitAfterGpuEndSum = 0;
        gpuSamples = 0;
        for (double& value : sequence) value = 0;
        sequenceCount = 0;
    }
};
PresentTiming timing;
}

/// @brief Native owner of matching-adapter DX12 resources, XeLL/FG contexts and triple allocator tracking.
/// @details Calls are serialized by the managed presenter owner. Shared image COM references remain caller-owned. releaseFailure preserves unsafe cleanup states instead of releasing remaining owners.
struct VulkanStoryXessFg {
    HMODULE fgModule = nullptr;
    HMODULE xellModule = nullptr;
    IDXGIFactory4* factory = nullptr;
    IDXGIAdapter1* adapter = nullptr;
    ID3D12Device* device = nullptr;
    ID3D12CommandQueue* queue = nullptr;
    ID3D12Fence* sharedFence = nullptr;
    IDXGISwapChain4* swapchain = nullptr;
    std::array<ID3D12CommandAllocator*, 3> allocators{};
    std::array<uint64_t, 3> allocatorCompletion{};
    ID3D12GraphicsCommandList* commandList = nullptr;
    ID3D12Fence* completionFence = nullptr;
    HANDLE completionEvent = nullptr;
    uint64_t completionValue = 0;
    uint32_t nextAllocator = 0;
    uint64_t lastDoneSignalled = 0;
    bool allowTearing = false;
    // Timing only (VULKANSTORY_XESS_FG_TIMING): two timestamps per allocator slot.
    ID3D12QueryHeap* queryHeap = nullptr;
    ID3D12Resource* queryReadback = nullptr;
    std::array<ID3D12CommandAllocator*, 3> postAllocators{};
    ID3D12GraphicsCommandList* postList = nullptr;
    uint64_t gpuFrequency = 0;
    std::array<int64_t, 3> presentEnter{};
    std::array<int64_t, 3> presentExit{};
    std::array<bool, 3> slotTimed{};
    xell_context_handle_t xell = nullptr;
    xefg_swapchain_handle_t fg = nullptr;
    uint32_t width = 0, height = 0;
    uint32_t maxInterpolations = 0, activeInterpolations = 0;
    bool started = false;
    int releaseFailure = 0;
    bool releasePrepared = false;

    decltype(&xellD3D12CreateContext) createXell = nullptr;
    decltype(&xellDestroyContext) destroyXell = nullptr;
    decltype(&xellSetSleepMode) setSleepMode = nullptr;
    decltype(&xellSleep) sleep = nullptr;
    decltype(&xellAddMarkerData) marker = nullptr;
    decltype(&xefgSwapChainD3D12CreateContext) createFg = nullptr;
    decltype(&xefgSwapChainDestroy) destroyFg = nullptr;
    decltype(&xefgSwapChainSetLatencyReduction) setLatency = nullptr;
    decltype(&xefgSwapChainD3D12InitFromSwapChainDesc) initSwapchain = nullptr;
    decltype(&xefgSwapChainD3D12GetSwapChainPtr) getSwapchain = nullptr;
    decltype(&xefgSwapChainSetEnabled) setEnabled = nullptr;
    decltype(&xefgSwapChainSetPresentId) setPresentId = nullptr;
    decltype(&xefgSwapChainSetNumInterpolatedFrames) setInterpolations = nullptr;
    decltype(&xefgSwapChainGetProperties) getProperties = nullptr;
    decltype(&xefgSwapChainSetUiCompositionState) setUi = nullptr;
    decltype(&xefgSwapChainD3D12TagFrameResource) tagResource = nullptr;
    decltype(&xefgSwapChainTagFrameConstants) tagConstants = nullptr;
    decltype(&xefgSwapChainGetLastPresentStatus) presentStatus = nullptr;
};

/// @brief Fixed-layout frame resource and camera record shared with the managed XeSS presenter.
/// @details All resource pointers borrow shared images in COMMON state. readyFenceValue follows Vulkan input copies; doneFenceValue follows queued DX12 copy/tag/present work and must exceed readyFenceValue.
struct VulkanStoryXessFrame {
    uint32_t frameId;
    uint32_t reset;
    uint32_t vsync;
    uint32_t reserved;
    ID3D12Resource* color;
    ID3D12Resource* depth;
    ID3D12Resource* motion;
    ID3D12Resource* hudless;
    ID3D12Resource* ui;
    float viewMatrix[16];
    float projectionMatrix[16];
    float jitterX, jitterY;
    float motionScaleX, motionScaleY;
    uint64_t readyFenceValue;
    uint64_t doneFenceValue;
};
static_assert(sizeof(VulkanStoryXessFrame) == 216);
static_assert(offsetof(VulkanStoryXessFrame, readyFenceValue) == 200);

/// @brief Locates this bridge's directory for sibling Intel runtime loading.
static std::wstring ModulePath() {
    HMODULE own = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
            GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            reinterpret_cast<LPCWSTR>(&ModulePath), &own)) return {};
    wchar_t path[MAX_PATH];
    DWORD length = GetModuleFileNameW(own, path, MAX_PATH);
    if (!length || length == MAX_PATH) return {};
    std::wstring result(path, length);
    auto separator = result.find_last_of(L"\\/");
    return separator == std::wstring::npos ? std::wstring{} : result.substr(0, separator + 1);
}

/// @brief Loads private XeSS-FG/XeLL modules and resolves the required SDK export set.
/// @details Acquired module handles belong to value and are released by checked context destruction.
static bool LoadRuntime(VulkanStoryXessFg* value) {
    std::wstring base = ModulePath();
    if (base.empty()) return false;
    value->fgModule = LoadLibraryExW((base + L"libxess_fg.dll").c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    value->xellModule = LoadLibraryExW((base + L"libxell.dll").c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!value->fgModule || !value->xellModule) return false;
#define LOAD(module, field, symbol) value->field = reinterpret_cast<decltype(value->field)>(GetProcAddress(value->module, #symbol))
    LOAD(xellModule, createXell, xellD3D12CreateContext);
    LOAD(xellModule, destroyXell, xellDestroyContext);
    LOAD(xellModule, setSleepMode, xellSetSleepMode);
    LOAD(xellModule, sleep, xellSleep);
    LOAD(xellModule, marker, xellAddMarkerData);
    LOAD(fgModule, createFg, xefgSwapChainD3D12CreateContext);
    LOAD(fgModule, destroyFg, xefgSwapChainDestroy);
    LOAD(fgModule, setLatency, xefgSwapChainSetLatencyReduction);
    LOAD(fgModule, initSwapchain, xefgSwapChainD3D12InitFromSwapChainDesc);
    LOAD(fgModule, getSwapchain, xefgSwapChainD3D12GetSwapChainPtr);
    LOAD(fgModule, setEnabled, xefgSwapChainSetEnabled);
    LOAD(fgModule, setPresentId, xefgSwapChainSetPresentId);
    LOAD(fgModule, setInterpolations, xefgSwapChainSetNumInterpolatedFrames);
    LOAD(fgModule, getProperties, xefgSwapChainGetProperties);
    LOAD(fgModule, setUi, xefgSwapChainSetUiCompositionState);
    LOAD(fgModule, tagResource, xefgSwapChainD3D12TagFrameResource);
    LOAD(fgModule, tagConstants, xefgSwapChainTagFrameConstants);
    LOAD(fgModule, presentStatus, xefgSwapChainGetLastPresentStatus);
#undef LOAD
    return value->createXell && value->destroyXell && value->setSleepMode &&
        value->sleep && value->marker && value->createFg && value->destroyFg &&
        value->setLatency && value->initSwapchain && value->getSwapchain &&
        value->setEnabled && value->setPresentId && value->setInterpolations &&
        value->getProperties &&
        value->setUi && value->tagResource && value->tagConstants &&
        value->presentStatus;
}

/// @brief Waits up to ten seconds for the tracked DX12 completion value.
/// @details A UINT64_MAX fence value records device removal as terminal failure rather than successful completion.
static int WaitForCompletion(VulkanStoryXessFg* value, uint64_t target) {
    if (value->releaseFailure != 0) return value->releaseFailure;
    if (!target) return 0;
    if (!value->completionFence) return value->releaseFailure = static_cast<int>(E_UNEXPECTED);
    uint64_t completed = value->completionFence->GetCompletedValue();
    if (completed == UINT64_MAX)
        return value->releaseFailure = static_cast<int>(DXGI_ERROR_DEVICE_REMOVED);
    if (completed >= target) return 0;
    if (!value->completionEvent) return -2;
    HRESULT result = value->completionFence->SetEventOnCompletion(
        target, value->completionEvent);
    if (FAILED(result)) return static_cast<int>(result);
    if (WaitForSingleObject(value->completionEvent, 10000) != WAIT_OBJECT_0) return -3;
    completed = value->completionFence->GetCompletedValue();
    if (completed == UINT64_MAX)
        return value->releaseFailure = static_cast<int>(DXGI_ERROR_DEVICE_REMOVED);
    return completed >= target ? 0 : -3;
}

/// @brief Waits for the context's most recent tracked DX12 completion value.
/// @return Zero when complete; -1 for a null context or a nonzero completion error.
extern "C" __declspec(dllexport) int VulkanStoryXessFgWaitIdle(VulkanStoryXessFg* value) {
    return value ? WaitForCompletion(value, value->completionValue) : -1;
}

/// @brief Disables active FG, drains DX12, releases the DXGI wrapper reference and destroys SDK consumers.
/// @details The presenter thread must be stopped and Vulkan users drained by the caller. On failure remaining context/resource owners stay attached.
static int PrepareDestroy(VulkanStoryXessFg* value) {
    if (!value) return 0;
    if (value->fg && value->started) {
        if (!value->setEnabled) return -1;
        int result = static_cast<int>(value->setEnabled(value->fg, 0));
        if (result < 0) return result;
    }
    int idle = VulkanStoryXessFgWaitIdle(value);
    if (idle != 0) return idle;
    // The SDK cannot destroy its context while our DXGI wrapper reference is
    // outstanding. The present thread is stopped and both queues are drained;
    // relinquish only this reference before destroying SDK consumers.
    Release(value->swapchain);
    // Destroy SDK consumers before the host retires shared input resources.
    // On failure leave the failed context and all remaining owners attached.
    if (value->fg) {
        if (!value->destroyFg) return -1;
        int result = static_cast<int>(value->destroyFg(value->fg));
        if (result < 0) return result;
        value->fg = nullptr;
    }
    if (value->xell) {
        if (!value->destroyXell) return -1;
        int result = static_cast<int>(value->destroyXell(value->xell));
        if (result < 0) return result;
        value->xell = nullptr;
    }
    value->started = false;
    return 0;
}

// Creation and its diagnostic read run on the same serialized host thread.
// Literal pointers remain valid until this bridge is unloaded.
static thread_local const char* lastCreateStage = "not attempted";
/// @brief Returns the diagnostic stage recorded by creation on this same host thread.
/// @details The returned literal pointer borrows bridge module lifetime and must not be freed.
extern "C" __declspec(dllexport) const char* VulkanStoryXessFgLastCreateStage() {
    return lastCreateStage;
}

/// @brief Performs checked SDK-consumer release preparation before shared input resources are destroyed.
/// @details A failed drain/destruction is persisted as releaseFailure, retaining the owner and preventing subsequent destructive cleanup.
/// @return Zero when prepared/null, otherwise the retained failure code.
extern "C" __declspec(dllexport) int VulkanStoryXessFgPrepareDestroy(VulkanStoryXessFg* value) {
    if (!value) return 0;
    if (value->releaseFailure != 0) return value->releaseFailure;
    if (value->releasePrepared) return 0;
    int result = PrepareDestroy(value);
    if (result != 0) value->releaseFailure = result;
    else value->releasePrepared = true;
    return result;
}

/// @brief Releases native/COM/modules and deletes the context only after checked preparation succeeds.
/// @details The caller must stop present work and drain Vulkan shared-image users first. Failed preparation leaves the value pointer owned and valid for retained-failure reporting.
/// @return Zero after destruction/null, otherwise the retained preparation failure.
extern "C" __declspec(dllexport) int VulkanStoryXessFgDestroyChecked(VulkanStoryXessFg* value) {
    if (!value) return 0;
    int prepared = VulkanStoryXessFgPrepareDestroy(value);
    if (prepared != 0) return prepared;
    Release(value->commandList);
    for (auto*& allocator : value->allocators) Release(allocator);
    Release(value->postList);
    for (auto*& allocator : value->postAllocators) Release(allocator);
    Release(value->queryReadback);
    Release(value->queryHeap);
    Release(value->completionFence);
    if (value->completionEvent) CloseHandle(value->completionEvent);
    Release(value->sharedFence);
    Release(value->queue);
    Release(value->device);
    Release(value->adapter);
    Release(value->factory);
    if (value->fgModule) FreeLibrary(value->fgModule);
    if (value->xellModule) FreeLibrary(value->xellModule);
    delete value;
    return 0;
}
/// @brief Legacy void destruction entry point that retains unsafe owners instead of freeing them.
/// @details New callers use DestroyChecked to observe failed drains or SDK destruction.
extern "C" __declspec(dllexport) void VulkanStoryXessFgDestroy(VulkanStoryXessFg* value) {
    // Retain the legacy ABI for native bring-up callers, but never free after
    // an unsuccessful drain or SDK destruction.
    VulkanStoryXessFgDestroyChecked(value);
}

// The 64-bit LUID is copied from VkPhysicalDeviceIDProperties::deviceLUID.
// The exact adapter match prevents cross-GPU resource handoffs.
/// @brief Creates DX12/XeLL/FG contexts on the exact adapter identified by the Vulkan device LUID.
/// @details Creation records thread-local stages. Failed initialization can attach a retained native owner to output when checked cleanup fails.
/// @param adapterLuid Nonzero Windows adapter LUID copied from Vulkan physical-device ID properties.
/// @param output Required owner slot; checked destruction owns final release.
/// @return Zero on success; local, HRESULT-derived or Intel SDK failure otherwise.
extern "C" __declspec(dllexport) int VulkanStoryXessFgCreate(uint64_t adapterLuid,
    VulkanStoryXessFg** output) {
    lastCreateStage = "argument validation";
    if (!output || !adapterLuid) return -1;
    *output = nullptr;
    lastCreateStage = "native context allocation";
    auto* value = new (std::nothrow) VulkanStoryXessFg();
    if (!value) return -2;
    int error = 0;
    do {
        lastCreateStage = "runtime DLL/export loading";
        if (!LoadRuntime(value)) { error = -3; break; }
        lastCreateStage = "CreateDXGIFactory2";
        if (FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&value->factory)))) { error = -4; break; }
        LUID luid{};
        luid.LowPart = static_cast<DWORD>(adapterLuid);
        luid.HighPart = static_cast<LONG>(adapterLuid >> 32);
        lastCreateStage = "EnumAdapterByLuid";
        if (FAILED(value->factory->EnumAdapterByLuid(luid, IID_PPV_ARGS(&value->adapter)))) {
            error = -5; break;
        }
        lastCreateStage = "D3D12CreateDevice";
        if (FAILED(D3D12CreateDevice(value->adapter, D3D_FEATURE_LEVEL_11_0,
                IID_PPV_ARGS(&value->device)))) { error = -6; break; }
        D3D12_COMMAND_QUEUE_DESC queue{};
        queue.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
        lastCreateStage = "CreateCommandQueue";
        if (FAILED(value->device->CreateCommandQueue(&queue,
                IID_PPV_ARGS(&value->queue)))) { error = -7; break; }
        lastCreateStage = "xellD3D12CreateContext";
        int xell = static_cast<int>(value->createXell(value->device, &value->xell));
        if (xell != 0 || !value->xell) { error = xell ? xell : -8; break; }
        lastCreateStage = "xefgSwapChainD3D12CreateContext";
        int fg = static_cast<int>(value->createFg(value->device, &value->fg));
        if (fg != 0 || !value->fg) { error = fg ? fg : -9; break; }
        if (std::getenv("VULKANSTORY_XESS_FG_LOG")) {
            using SetLogging = xefg_swapchain_result_t (*)(xefg_swapchain_handle_t,
                xefg_swapchain_logging_level_t, xefg_swapchain_app_log_callback_t, void*);
            auto setLogging = reinterpret_cast<SetLogging>(reinterpret_cast<void*>(
                GetProcAddress(value->fgModule, "xefgSwapChainSetLoggingCallback")));
            if (setLogging)
                setLogging(value->fg, XEFG_SWAPCHAIN_LOGGING_LEVEL_DEBUG,
                    [](const char* message, xefg_swapchain_logging_level_t level, void*) {
                        std::fprintf(stderr, "[xess-fg sdk %d] %s\n", static_cast<int>(level), message);
                        std::fflush(stderr);
                    }, nullptr);
        }
        lastCreateStage = "xefgSwapChainSetLatencyReduction";
        fg = static_cast<int>(value->setLatency(value->fg, value->xell));
        if (fg != 0) { error = fg; break; }
        xell_sleep_params_t mode{};
        mode.bLowLatencyMode = 1;
        lastCreateStage = "xellSetSleepMode";
        xell = static_cast<int>(value->setSleepMode(value->xell, &mode));
        if (xell != 0) { error = xell; break; }
    } while (false);
    if (error) {
        std::fprintf(stderr, "[xess-fg] initialization stage %s failed (%d)\n", lastCreateStage, error);
        int cleanup = VulkanStoryXessFgDestroyChecked(value);
        if (cleanup != 0) {
            *output = value;
            std::fprintf(stderr, "[xess-fg] initialization failed (%d), release failed (%d); native owner retained\n",
                error, cleanup);
        }
        return error;
    }
    *output = value;
    lastCreateStage = "ready";
    return 0;
}

/// @brief Queries the selected Vulkan device LUID and creates its matching native presentation context.
/// @details The Vulkan physical device is borrowed and must expose a valid Windows LUID.
/// @return Native creation result, or a negative argument/Vulkan-query availability code.
extern "C" __declspec(dllexport) int VulkanStoryXessFgCreateFromVulkan(
    VkPhysicalDevice physical, VulkanStoryXessFg** output) {
    lastCreateStage = "Vulkan adapter LUID lookup";
    if (!output) return -1;
    *output = nullptr;
    if (!physical) return -1;
    HMODULE loader = GetModuleHandleW(L"vulkan-1.dll");
    auto properties = loader ? reinterpret_cast<PFN_vkGetPhysicalDeviceProperties2>(
        GetProcAddress(loader, "vkGetPhysicalDeviceProperties2")) : nullptr;
    if (!properties) return -10;
    VkPhysicalDeviceIDProperties id{};
    id.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES;
    VkPhysicalDeviceProperties2 props{};
    props.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2;
    props.pNext = &id;
    properties(physical, &props);
    if (!id.deviceLUIDValid) return -11;
    uint64_t luid = 0;
    std::memcpy(&luid, id.deviceLUID, VK_LUID_SIZE);
    return VulkanStoryXessFgCreate(luid, output);
}

/// @brief Initializes the three-buffer XeSS DXGI swapchain and presentation command resources for a borrowed HWND.
/// @details The caller ensures this presenter is the window's sole swapchain owner. The initial vsync argument is reserved here; each frame supplies its actual Present interval.
/// @return Zero on successful startup; local, HRESULT or SDK failure otherwise.
extern "C" __declspec(dllexport) int VulkanStoryXessFgStart(VulkanStoryXessFg* value,
    HWND window, uint32_t width, uint32_t height, uint32_t vsync) {
    if (!value || !window || !width || !height || value->started) return -1;
    (void)vsync;
    IDXGIFactory5* factory5 = nullptr;
    if (SUCCEEDED(value->factory->QueryInterface(IID_PPV_ARGS(&factory5)))) {
        BOOL supported = FALSE;
        value->allowTearing = SUCCEEDED(factory5->CheckFeatureSupport(
            DXGI_FEATURE_PRESENT_ALLOW_TEARING, &supported, sizeof(supported))) && supported;
        Release(factory5);
    }
    DXGI_SWAP_CHAIN_DESC1 desc{};
    desc.Width = width;
    desc.Height = height;
    desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    desc.SampleDesc.Count = 1;
    desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.BufferCount = 3;
    desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    desc.Flags = value->allowTearing ? DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING : 0;
    xefg_swapchain_d3d12_init_params_t params{};
    params.maxInterpolatedFrames = XEFG_SWAPCHAIN_USE_MAX_SUPPORTED_INTERPOLATED_FRAMES;
    params.uiMode = XEFG_SWAPCHAIN_UI_MODE_HUDLESS_UITEXTURE;
    int code = static_cast<int>(value->initSwapchain(value->fg, window, &desc, nullptr,
        value->queue, value->factory, &params));
    if (code < 0) return code;
    xefg_swapchain_properties_t properties{};
    code = static_cast<int>(value->getProperties(value->fg, &properties));
    if (code < 0 || !properties.maxSupportedInterpolations)
        return code < 0 ? code : -4;
    value->maxInterpolations = properties.maxSupportedInterpolations;
    code = static_cast<int>(value->getSwapchain(value->fg,
        __uuidof(IDXGISwapChain4), reinterpret_cast<void**>(&value->swapchain)));
    if (code < 0 || !value->swapchain) return code < 0 ? code : -2;
    // HUDLESS_UITEXTURE selects the composition method, but the SDK leaves
    // composition disabled until this separate switch is enabled.
    code = static_cast<int>(value->setUi(value->fg,
        XEFG_SWAPCHAIN_UI_COMPOSITION_STATE_ENABLED));
    if (code < 0) return code;
    HRESULT result = value->device->CreateFence(0, D3D12_FENCE_FLAG_NONE,
        IID_PPV_ARGS(&value->completionFence));
    if (FAILED(result)) return static_cast<int>(result);
    value->completionEvent = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!value->completionEvent) return -3;
    for (auto*& allocator : value->allocators) {
        result = value->device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,
            IID_PPV_ARGS(&allocator));
        if (FAILED(result)) return static_cast<int>(result);
    }
    result = value->device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT,
        value->allocators[0], nullptr, IID_PPV_ARGS(&value->commandList));
    if (FAILED(result)) return static_cast<int>(result);
    result = value->commandList->Close();
    if (FAILED(result)) return static_cast<int>(result);
    value->width = width;
    value->height = height;
    value->started = true;
    return 0;
}

/// @brief Forwards frame-generation enablement for an already started native presenter.
/// @return SDK result, or -1 without a started FG context.
extern "C" __declspec(dllexport) int VulkanStoryXessFgSetEnabled(VulkanStoryXessFg* value,
    uint32_t enabled) {
    return value && value->fg && value->started ?
        static_cast<int>(value->setEnabled(value->fg, enabled ? 1 : 0)) : -1;
}

/// @brief Clamps interpolation count to the started runtime's supported range and avoids redundant SDK updates.
/// @details Outputs describe the clamped request and maximum. activeInterpolations updates only for nonnegative SDK results.
/// @param value Borrowed started native owner.
/// @param requested Requested generated frames per rendered frame; clamped to one through the SDK limit.
/// @param effective Required output for the clamped count.
/// @param maximum Required output for the native interpolation limit.
/// @return SDK result, zero for an unchanged count, or -1 for missing prerequisites.
extern "C" __declspec(dllexport) int VulkanStoryXessFgSetGeneratedFrames(
    VulkanStoryXessFg* value, uint32_t requested, uint32_t* effective, uint32_t* maximum) {
    if (!value || !value->fg || !value->started || !effective || !maximum ||
        !value->maxInterpolations) return -1;
    *maximum = value->maxInterpolations;
    *effective = requested < 1 ? 1 :
        requested > value->maxInterpolations ? value->maxInterpolations : requested;
    if (*effective == value->activeInterpolations) return 0;
    int code = static_cast<int>(value->setInterpolations(value->fg, *effective));
    if (code >= 0) value->activeInterpolations = *effective;
    return code;
}

// VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT is a handle to a D3D12
// committed resource created by ID3D12Device::CreateSharedHandle. Vulkan imports
// this handle; it is not a handle exported from an arbitrary Vulkan allocation.
/// @brief Creates a committed DX12 texture and NT handle for dedicated Vulkan import.
/// @details Resources are created in COMMON state with the supported Vulkan/DXGI format mapping.
/// @param value Borrowed native DX12 owner.
/// @param width Nonzero image width.
/// @param height Nonzero image height.
/// @param vkFormat Supported Vulkan format translated to DXGI.
/// @param sharedHandle Caller-owned NT handle; close it after import/use.
/// @param resource Caller-owned COM reference; release after both APIs finish.
/// @return Zero on success; outputs are cleared before creation. Unsupported formats/local/HRESULT failures are nonzero.
extern "C" __declspec(dllexport) int VulkanStoryXessFgCreateSharedImage(VulkanStoryXessFg* value,
    uint32_t width, uint32_t height, uint32_t vkFormat, HANDLE* sharedHandle,
    ID3D12Resource** resource) {
    if (!value || !value->device || !width || !height || !sharedHandle || !resource)
        return -1;
    *sharedHandle = nullptr;
    *resource = nullptr;
    DXGI_FORMAT format = DXGI_FORMAT_UNKNOWN;
    D3D12_RESOURCE_FLAGS flags = D3D12_RESOURCE_FLAG_NONE;
    switch (static_cast<VkFormat>(vkFormat)) {
        case VK_FORMAT_R8G8B8A8_UNORM: format = DXGI_FORMAT_R8G8B8A8_UNORM; break;
        case VK_FORMAT_B8G8R8A8_UNORM: format = DXGI_FORMAT_B8G8R8A8_UNORM; break;
        case VK_FORMAT_R16G16_SFLOAT: format = DXGI_FORMAT_R16G16_FLOAT; break;
        case VK_FORMAT_R16G16B16A16_SFLOAT: format = DXGI_FORMAT_R16G16B16A16_FLOAT; break;
        case VK_FORMAT_R32_SFLOAT: format = DXGI_FORMAT_R32_FLOAT; break;
        case VK_FORMAT_D32_SFLOAT:
            format = DXGI_FORMAT_D32_FLOAT;
            flags = D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL;
            break;
        default: return -2;
    }
    D3D12_HEAP_PROPERTIES heap{};
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{};
    desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = width;
    desc.Height = height;
    desc.DepthOrArraySize = 1;
    desc.MipLevels = 1;
    desc.Format = format;
    desc.SampleDesc.Count = 1;
    desc.Layout = D3D12_TEXTURE_LAYOUT_UNKNOWN;
    desc.Flags = flags;
    HRESULT result = value->device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_SHARED,
        &desc, D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(resource));
    if (FAILED(result)) return static_cast<int>(result);
    result = value->device->CreateSharedHandle(*resource, nullptr, GENERIC_ALL,
        nullptr, sharedHandle);
    if (FAILED(result)) {
        Release(*resource);
        return static_cast<int>(result);
    }
    return 0;
}

/// @brief Releases a caller-owned shared-image COM reference; null is accepted.
/// @details No waiting occurs; the host must first complete Vulkan import users and DX12 consumers.
extern "C" __declspec(dllexport) void VulkanStoryXessFgReleaseImage(ID3D12Resource* resource) {
    Release(resource);
}

// The caller closes the NT handle after a permanent Vulkan semaphore import.
// The bridge retains the ID3D12Fence for its command queue's GPU-side waits.
/// @brief Creates or reuses the native-owned DX12 shared fence and exports a caller-owned NT handle.
/// @details The importer closes the handle after permanent Vulkan timeline-semaphore import; the bridge keeps the DX12 fence for GPU queue waits/signals.
/// @return Zero on success, or a local/HRESULT error.
extern "C" __declspec(dllexport) int VulkanStoryXessFgCreateSharedFence(
    VulkanStoryXessFg* value, HANDLE* sharedHandle) {
    if (!value || !value->device || !sharedHandle) return -1;
    *sharedHandle = nullptr;
    if (!value->sharedFence) {
        HRESULT result = value->device->CreateFence(0, D3D12_FENCE_FLAG_SHARED,
            IID_PPV_ARGS(&value->sharedFence));
        if (FAILED(result)) return static_cast<int>(result);
    }
    HRESULT result = value->device->CreateSharedHandle(value->sharedFence, nullptr,
        GENERIC_ALL, nullptr, sharedHandle);
    return FAILED(result) ? static_cast<int>(result) : 0;
}

/// @brief Queues a DX12 GPU wait for the borrowed shared-fence value.
/// @details This returns after enqueuing the wait; it does not synchronously wait on the CPU.
/// @return Zero when enqueued, otherwise local/HRESULT failure.
extern "C" __declspec(dllexport) int VulkanStoryXessFgWaitSharedFence(
    VulkanStoryXessFg* value, uint64_t fenceValue) {
    if (!value || !value->queue || !value->sharedFence || !fenceValue) return -1;
    HRESULT result = value->queue->Wait(value->sharedFence, fenceValue);
    return FAILED(result) ? static_cast<int>(result) : 0;
}

/// @brief Queues a DX12 shared-fence signal behind prior queue work.
/// @details The caller must reserve ordered values and must not equate a successful enqueue with immediate GPU completion.
/// @return Zero when enqueued, otherwise local/HRESULT failure.
extern "C" __declspec(dllexport) int VulkanStoryXessFgSignalSharedFence(
    VulkanStoryXessFg* value, uint64_t fenceValue) {
    if (!value || !value->queue || !value->sharedFence || !fenceValue) return -1;
    HRESULT result = value->queue->Signal(value->sharedFence, fenceValue);
    return FAILED(result) ? static_cast<int>(result) : 0;
}

/// @brief Records an all-subresource state transition for a borrowed DX12 resource.
static void Transition(ID3D12GraphicsCommandList* list, ID3D12Resource* resource,
    D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition.pResource = resource;
    barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barrier.Transition.StateBefore = before;
    barrier.Transition.StateAfter = after;
    list->ResourceBarrier(1, &barrier);
}

namespace {
/// @brief Lazily creates opt-in timestamp queries, readback storage and post-present command lists.
/// @details Objects remain owned by the context and are released during checked destruction; a false result disables that timing sample.
bool EnsureGpuTiming(VulkanStoryXessFg* value) {
    if (value->postList) return true;
    D3D12_QUERY_HEAP_DESC heap{};
    heap.Type = D3D12_QUERY_HEAP_TYPE_TIMESTAMP;
    heap.Count = 9;
    if (FAILED(value->device->CreateQueryHeap(&heap, IID_PPV_ARGS(&value->queryHeap)))) return false;
    D3D12_HEAP_PROPERTIES readbackHeap{};
    readbackHeap.Type = D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC buffer{};
    buffer.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    buffer.Width = 9 * sizeof(uint64_t);
    buffer.Height = 1;
    buffer.DepthOrArraySize = 1;
    buffer.MipLevels = 1;
    buffer.SampleDesc.Count = 1;
    buffer.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    if (FAILED(value->device->CreateCommittedResource(&readbackHeap, D3D12_HEAP_FLAG_NONE, &buffer,
            D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&value->queryReadback)))) return false;
    for (auto*& allocator : value->postAllocators)
        if (FAILED(value->device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,
                IID_PPV_ARGS(&allocator)))) return false;
    if (FAILED(value->device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT,
            value->postAllocators[0], nullptr, IID_PPV_ARGS(&value->postList)))) return false;
    value->postList->Close();
    return SUCCEEDED(value->queue->GetTimestampFrequency(&value->gpuFrequency)) && value->gpuFrequency;
}

// The slot's previous present has completed on the DX12 queue: its two
// timestamps are final and go on the CPU clock through a fresh calibration.
/// @brief Reads completed slot timestamps and calibrates them to the CPU performance-counter clock.
/// @details The caller first waits for the slot's previous DX12 completion before reusing query/allocator storage.
void HarvestGpuTiming(VulkanStoryXessFg* value, uint32_t slot) {
    if (!value->slotTimed[slot]) return;
    value->slotTimed[slot] = false;
    uint64_t* mapped = nullptr;
    D3D12_RANGE range{slot * 3 * sizeof(uint64_t), (slot * 3 + 3) * sizeof(uint64_t)};
    if (FAILED(value->queryReadback->Map(0, &range, reinterpret_cast<void**>(&mapped)))) return;
    uint64_t gpuStart = mapped[slot * 3], gpuCopiesEnd = mapped[slot * 3 + 1], gpuEnd = mapped[slot * 3 + 2];
    D3D12_RANGE none{0, 0};
    value->queryReadback->Unmap(0, &none);
    uint64_t gpuCalibration = 0, cpuCalibration = 0;
    if (gpuEnd <= gpuStart || FAILED(value->queue->GetClockCalibration(&gpuCalibration, &cpuCalibration))) return;
    auto toCpu = [&](uint64_t gpu) {
        return static_cast<double>(cpuCalibration) -
            (static_cast<double>(gpuCalibration) - static_cast<double>(gpu)) * timing.frequency / value->gpuFrequency;
    };
    double msPerTick = 1000.0 / timing.frequency;
    double duration = (gpuEnd - gpuStart) * 1000.0 / value->gpuFrequency;
    TimingGuard guard;
    timing.gpuSum += duration;
    if (gpuCopiesEnd >= gpuStart && gpuCopiesEnd <= gpuEnd)
        timing.copiesSum += (gpuCopiesEnd - gpuStart) * 1000.0 / value->gpuFrequency;
    if (duration > timing.gpuMax) timing.gpuMax = duration;
    timing.startAfterEnterSum += (toCpu(gpuStart) - value->presentEnter[slot]) * msPerTick;
    timing.exitAfterGpuEndSum += (value->presentExit[slot] - toCpu(gpuEnd)) * msPerTick;
    timing.gpuSamples++;
}
}

// The Vulkan queue signals readyFenceValue after releasing shared-image
// ownership. The SDK records its ONLY_NOW input copies into this command list;
// doneFenceValue is signalled only after its proxy Present has queued work.
/// @brief Records input copies/tags and performs native proxy presentation for one prepared frame.
/// @details After submission, shared done and local completion signals track queued work even when Present fails. Input resources are tagged ONLY_NOW on the recording command list.
/// @return Zero when presentation/status querying succeeds, otherwise a local, HRESULT or Intel SDK result.
static int PresentFrame(VulkanStoryXessFg* value,
    const VulkanStoryXessFrame* frame, uint32_t* framesPresented,
    int* frameGenResult, uint32_t* frameGenEnabled) {
    if (framesPresented) *framesPresented = 0;
    if (frameGenResult) *frameGenResult = 0;
    if (frameGenEnabled) *frameGenEnabled = 0;
    if (!value || !value->started || !frame || !framesPresented ||
        !frameGenResult || !frameGenEnabled || !value->sharedFence ||
        !frame->frameId || !frame->readyFenceValue ||
        frame->doneFenceValue <= frame->readyFenceValue ||
        !frame->color || !frame->depth || !frame->motion ||
        !frame->hudless || !frame->ui) return -1;

    D3D12_RESOURCE_DESC color = frame->color->GetDesc();
    D3D12_RESOURCE_DESC hudless = frame->hudless->GetDesc();
    D3D12_RESOURCE_DESC ui = frame->ui->GetDesc();
    D3D12_RESOURCE_DESC motion = frame->motion->GetDesc();
    D3D12_RESOURCE_DESC depth = frame->depth->GetDesc();
    if (color.Width != value->width || color.Height != value->height ||
        color.Format != DXGI_FORMAT_B8G8R8A8_UNORM ||
        hudless.Width != value->width || hudless.Height != value->height ||
        hudless.Format != color.Format || ui.Width != value->width ||
        ui.Height != value->height || ui.Format != color.Format ||
        motion.Width != depth.Width || motion.Height != depth.Height ||
        motion.Format != DXGI_FORMAT_R16G16_FLOAT ||
        depth.Format != DXGI_FORMAT_D32_FLOAT) return -2;

    const bool timed = timing.enabled;
    int64_t phaseStart = timed ? timing.Now() : 0;
    uint32_t slot = value->nextAllocator++ % static_cast<uint32_t>(value->allocators.size());
    uint64_t completion = value->allocatorCompletion[slot];
    int idle = WaitForCompletion(value, completion);
    if (idle != 0) return idle;
    if (timed) { int64_t now = timing.Now(); timing.Add(kAllocatorWait, phaseStart, now); phaseStart = now; }
    const bool gpuTimed = timed && EnsureGpuTiming(value);
    if (gpuTimed) HarvestGpuTiming(value, slot);
    HRESULT result = value->allocators[slot]->Reset();
    if (FAILED(result)) return static_cast<int>(result);
    result = value->commandList->Reset(value->allocators[slot], nullptr);
    if (FAILED(result)) return static_cast<int>(result);
    if (gpuTimed) value->commandList->EndQuery(value->queryHeap, D3D12_QUERY_TYPE_TIMESTAMP, slot * 3);

    std::array<ID3D12Resource*, 5> inputs = {
        frame->color, frame->depth, frame->motion, frame->hudless, frame->ui
    };
    for (ID3D12Resource* input : inputs)
        Transition(value->commandList, input, D3D12_RESOURCE_STATE_COMMON,
            D3D12_RESOURCE_STATE_COPY_SOURCE);
    ID3D12Resource* backbuffer = nullptr;
    uint32_t backbufferIndex = value->swapchain->GetCurrentBackBufferIndex();
    result = value->swapchain->GetBuffer(backbufferIndex, IID_PPV_ARGS(&backbuffer));
    if (FAILED(result)) { value->commandList->Close(); return static_cast<int>(result); }
    Transition(value->commandList, backbuffer, D3D12_RESOURCE_STATE_PRESENT,
        D3D12_RESOURCE_STATE_COPY_DEST);
    value->commandList->CopyResource(backbuffer, frame->color);

    struct Tag { ID3D12Resource* resource; xefg_swapchain_resource_type_t type; };
    std::array<Tag, 4> tags = {{
        {frame->depth, XEFG_SWAPCHAIN_RES_DEPTH},
        {frame->motion, XEFG_SWAPCHAIN_RES_MOTION_VECTOR},
        {frame->hudless, XEFG_SWAPCHAIN_RES_HUDLESS_COLOR},
        {frame->ui, XEFG_SWAPCHAIN_RES_UI},
    }};
    for (const Tag& tag : tags) {
        D3D12_RESOURCE_DESC desc = tag.resource->GetDesc();
        xefg_swapchain_d3d12_resource_data_t data{};
        data.type = tag.type;
        data.validity = XEFG_SWAPCHAIN_RV_ONLY_NOW;
        data.resourceSize = {static_cast<uint32_t>(desc.Width), desc.Height};
        data.pResource = tag.resource;
        data.incomingState = D3D12_RESOURCE_STATE_COPY_SOURCE;
        int code = static_cast<int>(value->tagResource(value->fg, value->commandList,
            frame->frameId, &data));
        if (code < 0) {
            value->commandList->Close();
            Release(backbuffer);
            return code;
        }
    }
    for (ID3D12Resource* input : inputs)
        Transition(value->commandList, input, D3D12_RESOURCE_STATE_COPY_SOURCE,
            D3D12_RESOURCE_STATE_COMMON);
    Transition(value->commandList, backbuffer, D3D12_RESOURCE_STATE_COPY_DEST,
        D3D12_RESOURCE_STATE_PRESENT);
    Release(backbuffer);
    if (gpuTimed) value->commandList->EndQuery(value->queryHeap, D3D12_QUERY_TYPE_TIMESTAMP, slot * 3 + 1);
    result = value->commandList->Close();
    if (FAILED(result)) return static_cast<int>(result);

    xefg_swapchain_frame_constant_data_t constants{};
    std::memcpy(constants.viewMatrix, frame->viewMatrix, sizeof(constants.viewMatrix));
    std::memcpy(constants.projectionMatrix, frame->projectionMatrix,
        sizeof(constants.projectionMatrix));
    constants.jitterOffsetX = frame->jitterX;
    constants.jitterOffsetY = frame->jitterY;
    constants.motionVectorScaleX = frame->motionScaleX;
    constants.motionVectorScaleY = frame->motionScaleY;
    constants.resetHistory = frame->reset;
    int code = static_cast<int>(value->tagConstants(value->fg, frame->frameId, &constants));
    if (code < 0) return code;
    if (timed) { int64_t now = timing.Now(); timing.Add(kRecord, phaseStart, now); phaseStart = now; }
    result = value->queue->Wait(value->sharedFence, frame->readyFenceValue);
    if (FAILED(result)) return static_cast<int>(result);
    ID3D12CommandList* lists[] = {value->commandList};
    value->queue->ExecuteCommandLists(1, lists);
    int presentIdCode = static_cast<int>(value->setPresentId(value->fg, frame->frameId));
    uint32_t flags = !frame->vsync && value->allowTearing ? DXGI_PRESENT_ALLOW_TEARING : 0;
    if (timed) { int64_t now = timing.Now(); timing.Add(kSubmit, phaseStart, now); phaseStart = now; }
    if (gpuTimed) value->presentEnter[slot] = timing.Now();
    result = presentIdCode >= 0 ? value->swapchain->Present(frame->vsync ? 1 : 0, flags) : E_FAIL;
    if (timed) { int64_t now = timing.Now(); timing.Add(kPresent, phaseStart, now); phaseStart = now; }
    if (gpuTimed) {
        value->presentExit[slot] = timing.Now();
        if (SUCCEEDED(value->postAllocators[slot]->Reset()) &&
            SUCCEEDED(value->postList->Reset(value->postAllocators[slot], nullptr))) {
            value->postList->EndQuery(value->queryHeap, D3D12_QUERY_TYPE_TIMESTAMP, slot * 3 + 2);
            value->postList->ResolveQueryData(value->queryHeap, D3D12_QUERY_TYPE_TIMESTAMP, slot * 3, 3,
                value->queryReadback, slot * 3 * sizeof(uint64_t));
            if (SUCCEEDED(value->postList->Close())) {
                ID3D12CommandList* post[] = {value->postList};
                value->queue->ExecuteCommandLists(1, post);
                value->slotTimed[slot] = true;
            }
        }
    }
    // Even a failed present must release Vulkan's shared images after the
    // already-submitted copy/tag list has finished on this queue.
    HRESULT handoff = value->queue->Signal(value->sharedFence, frame->doneFenceValue);
    if (SUCCEEDED(handoff)) value->lastDoneSignalled = frame->doneFenceValue;
    uint64_t finished = ++value->completionValue;
    HRESULT retire = value->queue->Signal(value->completionFence, finished);
    value->allocatorCompletion[slot] = finished;
    if (FAILED(retire)) value->releaseFailure = static_cast<int>(retire);
    // Diagnostics (VULKANSTORY_XESS_FG_SERIALIZE): finish this present's DX12 work before
    // returning, so its GPU time is measured without the Vulkan queue competing.
    static const bool serialize = std::getenv("VULKANSTORY_XESS_FG_SERIALIZE") != nullptr;
    if (serialize && SUCCEEDED(retire) &&
        SUCCEEDED(value->completionFence->SetEventOnCompletion(finished, value->completionEvent)))
        WaitForSingleObject(value->completionEvent, 10000);
    if (FAILED(handoff)) return static_cast<int>(handoff);
    if (FAILED(retire)) return value->releaseFailure;
    if (presentIdCode < 0) return presentIdCode;
    if (FAILED(result)) return static_cast<int>(result);
    xefg_swapchain_present_status_t status{};
    code = static_cast<int>(value->presentStatus(value->fg, &status));
    if (code < 0) return code;
    *framesPresented = status.framesPresented;
    *frameGenResult = static_cast<int>(status.frameGenResult);
    *frameGenEnabled = status.isFrameGenEnabled;
    if (timed) { timing.Add(kRetire, phaseStart, timing.Now()); timing.EndPresent(); }
    return 0;
}

// Vulkan may already wait on doneFenceValue (the next frame's submission is gated on
// it), so every call signals it, failed ones included. A failed call first waits for
// readyFenceValue on this queue: the shared timeline must never move backwards.
/// @brief Presents a prepared frame and preserves the ordered shared-fence handoff on recoverable failure.
/// @details When the normal path did not signal the requested done value, the wrapper first queues a wait for readiness, then queues done and local completion signals. Failed signals retain a terminal release error; no CPU-side completion value is fabricated.
/// @param value Borrowed started native presenter; host calls are serialized.
/// @param frame Borrowed matching resources/constants with ordered ready/done values.
/// @param framesPresented Required native status output; zeroed before presentation.
/// @param frameGenResult Required interpolation-result output, separate from this function result.
/// @param frameGenEnabled Required native FG enablement-status output.
/// @return Zero on successful presentation/status query; local/HRESULT/SDK error otherwise.
extern "C" __declspec(dllexport) int VulkanStoryXessFgPresent(VulkanStoryXessFg* value,
    const VulkanStoryXessFrame* frame, uint32_t* framesPresented,
    int* frameGenResult, uint32_t* frameGenEnabled) {
    int code = PresentFrame(value, frame, framesPresented, frameGenResult, frameGenEnabled);
    if (value && frame && value->queue && value->sharedFence &&
        frame->doneFenceValue > value->lastDoneSignalled &&
        frame->doneFenceValue > frame->readyFenceValue) {
        HRESULT wait = value->queue->Wait(value->sharedFence, frame->readyFenceValue);
        if (FAILED(wait)) return value->releaseFailure = static_cast<int>(wait);
        uint64_t finished = ++value->completionValue;
        HRESULT handoff = value->queue->Signal(value->sharedFence, frame->doneFenceValue);
        if (SUCCEEDED(handoff)) value->lastDoneSignalled = frame->doneFenceValue;
        HRESULT retire = value->queue->Signal(value->completionFence, finished);
        if (FAILED(retire)) return value->releaseFailure = static_cast<int>(retire);
        if (FAILED(handoff)) return value->releaseFailure = static_cast<int>(handoff);
    }
    return code;
}

/// @brief Forwards XeLL sleep-mode and minimum-interval options.
/// @details The host must apply SDK options at an appropriate idle lifecycle boundary; this function does not drain the GPU.
/// @param value Borrowed native owner with a live XeLL context.
/// @param minimumIntervalUs Minimum frame interval in microseconds, or zero for no cap.
/// @param enabled Nonzero enables XeLL low-latency mode.
/// @return SDK result, or -1 without a XeLL context.
extern "C" __declspec(dllexport) int VulkanStoryXessFgSetLatencyMode(VulkanStoryXessFg* value,
    uint32_t minimumIntervalUs, uint32_t enabled) {
    if (!value || !value->xell) return -1;
    xell_sleep_params_t mode{};
    mode.minimumIntervalUs = minimumIntervalUs;
    mode.bLowLatencyMode = enabled ? 1 : 0;
    return static_cast<int>(value->setSleepMode(value->xell, &mode));
}

/// @brief Invokes XeLL sleep for the supplied frame before host input polling.
/// @details Optional timing measures CPU call duration only.
/// @return SDK result, or -1 without a XeLL context.
extern "C" __declspec(dllexport) int VulkanStoryXessFgSleep(VulkanStoryXessFg* value,
    uint32_t frameId) {
    if (!value || !value->xell) return -1;
    int64_t start = timing.enabled ? timing.Now() : 0;
    int result = static_cast<int>(value->sleep(value->xell, frameId));
    if (timing.enabled) timing.Add(kSleep, start, timing.Now());
    return result;
}

/// @brief Emits the supplied XeLL marker for the matching frame identity.
/// @return SDK result, or -1 without a XeLL context.
extern "C" __declspec(dllexport) int VulkanStoryXessFgMarker(VulkanStoryXessFg* value,
    uint32_t frameId, xell_latency_marker_type_t marker) {
    return value && value->xell ?
        static_cast<int>(value->marker(value->xell, frameId, marker)) : -1;
}
