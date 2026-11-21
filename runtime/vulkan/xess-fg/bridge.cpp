// XeSS-FG 3.0.2 DX12 presentation bridge. The integrated host supplies the
// SDK directory. The standalone bridge loads the DLLs beside itself.
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
#define RSF_XESS_BRIDGE_BUILD
#include "session_abi.h"
#include "diagnostic-capture.h"

/// @brief Releases a local COM reference and clears its pointer without GPU waiting.
template<typename T> static void Release(T*& value) {
    if (value) { value->Release(); value = nullptr; }
}

// Opt-in present-phase timing (VULKANSTORY_XESS_FG_TIMING=1): mean and max milliseconds
// per phase, written to stderr every 120 presents. Diagnostics only.
namespace {
/// @brief CPU presentation-phase indices used only by opt-in XeSS diagnostics.
enum TimingPhase { kAllocatorWait, kRecord, kSubmit, kPresent, kRetire, kSleep, kPhaseCount };
static_assert(kAllocatorWait == 0 && kRecord == 1 && kSubmit == 2 && kPresent == 3 && kRetire == 4);
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

// Typed storage for the Rust session and its presentation leaves. The public
// handle may remain live after failed retirement; only the session frees it.
struct VulkanStoryXessFg {
    rsf_xess_bridge_session* session = nullptr;
    rsf_xess_capture::Capture capture;
    IDXGIFactory4* factory = nullptr;
    IDXGIAdapter1* adapter = nullptr;
    ID3D12Device* device = nullptr;
    ID3D12CommandQueue* queue = nullptr;
    ID3D12Fence* sharedFence = nullptr;
    IDXGISwapChain4* swapchain = nullptr;
    std::array<ID3D12CommandAllocator*, RSF_XESS_BRIDGE_ALLOCATOR_COUNT> allocators{};
    ID3D12GraphicsCommandList* commandList = nullptr;
    ID3D12Fence* completionFence = nullptr;
    bool allowTearing = false;
    // Timing only (VULKANSTORY_XESS_FG_TIMING): two timestamps per allocator slot.
    ID3D12QueryHeap* queryHeap = nullptr;
    ID3D12Resource* queryReadback = nullptr;
    std::array<ID3D12CommandAllocator*, RSF_XESS_BRIDGE_ALLOCATOR_COUNT> postAllocators{};
    ID3D12GraphicsCommandList* postList = nullptr;
    uint64_t gpuFrequency = 0;
    std::array<int64_t, RSF_XESS_BRIDGE_ALLOCATOR_COUNT> presentEnter{};
    std::array<int64_t, RSF_XESS_BRIDGE_ALLOCATOR_COUNT> presentExit{};
    std::array<bool, RSF_XESS_BRIDGE_ALLOCATOR_COUNT> slotTimed{};
    xell_context_handle_t xell = nullptr;
    xefg_swapchain_handle_t fg = nullptr;
    uint32_t activeInterpolations = 0;

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
static_assert(sizeof(VulkanStoryXessFrame) == 216 && alignof(VulkanStoryXessFrame) == 8);
static_assert(offsetof(VulkanStoryXessFrame, color) == 16);
static_assert(offsetof(VulkanStoryXessFrame, viewMatrix) == 56);
static_assert(offsetof(VulkanStoryXessFrame, readyFenceValue) == 200);
static_assert(sizeof(rsf_xess_bridge_frame) == 216 && alignof(rsf_xess_bridge_frame) == 8);
static_assert(offsetof(rsf_xess_bridge_frame, color) == 24);
static_assert(offsetof(rsf_xess_bridge_frame, view_matrix) == 56);
static_assert(offsetof(rsf_xess_bridge_frame, ready_fence_value) == 200);
static_assert(sizeof(rsf_xess_bridge_present_status) == 24);
static_assert(offsetof(rsf_xess_bridge_present_status, frames_presented) == 8);
static_assert(offsetof(rsf_xess_bridge_present_status, reserved) == 20);

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

// Transfer each loaded module to the session, including on partial failure.
static int32_t LoadRuntime(VulkanStoryXessFg* value, const uint16_t* directory,
    rsf_xess_bridge_modules* modules) noexcept try {
    std::wstring base = directory ? reinterpret_cast<const wchar_t*>(directory) : ModulePath();
    if (base.empty()) return -3;
    if (base.back() != L'\\' && base.back() != L'/') base += L'\\';
    auto fgModule = LoadLibraryExW((base + L"libxess_fg.dll").c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    modules->fg = fgModule;
    auto xellModule = LoadLibraryExW((base + L"libxell.dll").c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    modules->xell = xellModule;
    if (!fgModule || !xellModule) return -3;
#define LOAD(module, field, symbol) value->field = reinterpret_cast<decltype(value->field)>(GetProcAddress(module, #symbol))
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
        value->presentStatus ? 0 : -3;
} catch (...) {
    return -3;
}

static VulkanStoryXessFg* Allocate(rsf_xess_bridge_session* session) noexcept try {
    auto* value = new (std::nothrow) VulkanStoryXessFg();
    if (value) value->session = session;
    return value;
} catch (...) {
    return nullptr;
}

static void ReleaseNative(VulkanStoryXessFg* value) {
    Release(value->commandList);
    for (auto*& allocator : value->allocators) Release(allocator);
    Release(value->postList);
    for (auto*& allocator : value->postAllocators) Release(allocator);
    Release(value->queryReadback);
    Release(value->queryHeap);
    Release(value->completionFence);
    Release(value->sharedFence);
    Release(value->queue);
    Release(value->device);
    Release(value->adapter);
    Release(value->factory);
    delete value;
}

static int32_t CreateFactory(VulkanStoryXessFg* value) {
    return FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&value->factory))) ? -4 : 0;
}

static int32_t FindAdapter(VulkanStoryXessFg* value, uint64_t adapterLuid) {
    LUID luid{};
    luid.LowPart = static_cast<DWORD>(adapterLuid);
    luid.HighPart = static_cast<LONG>(adapterLuid >> 32);
    return FAILED(value->factory->EnumAdapterByLuid(luid, IID_PPV_ARGS(&value->adapter))) ? -5 : 0;
}

static int32_t CreateDevice(VulkanStoryXessFg* value) {
    return FAILED(D3D12CreateDevice(value->adapter, D3D_FEATURE_LEVEL_11_0,
        IID_PPV_ARGS(&value->device))) ? -6 : 0;
}

static int32_t CreateQueue(VulkanStoryXessFg* value) {
    D3D12_COMMAND_QUEUE_DESC queue{};
    queue.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    return FAILED(value->device->CreateCommandQueue(&queue, IID_PPV_ARGS(&value->queue))) ? -7 : 0;
}

static int32_t CreateXell(VulkanStoryXessFg* value) {
    int code = static_cast<int>(value->createXell(value->device, &value->xell));
    return code == 0 && !value->xell ? -8 : code;
}

static int32_t CreateFg(VulkanStoryXessFg* value, void* module) {
    int code = static_cast<int>(value->createFg(value->device, &value->fg));
    if (code != 0 || !value->fg) return code != 0 ? code : -9;
    if (std::getenv("VULKANSTORY_XESS_FG_LOG")) {
        using SetLogging = xefg_swapchain_result_t (*)(xefg_swapchain_handle_t,
            xefg_swapchain_logging_level_t, xefg_swapchain_app_log_callback_t, void*);
        auto setLogging = reinterpret_cast<SetLogging>(reinterpret_cast<void*>(
            GetProcAddress(static_cast<HMODULE>(module), "xefgSwapChainSetLoggingCallback")));
        if (setLogging)
            setLogging(value->fg, XEFG_SWAPCHAIN_LOGGING_LEVEL_DEBUG,
                [](const char* message, xefg_swapchain_logging_level_t level, void*) {
                    std::fprintf(stderr, "[xess-fg sdk %d] %s\n", static_cast<int>(level), message);
                    std::fflush(stderr);
                }, nullptr);
    }
    return 0;
}

static void LogCreateError(const char* stage, int32_t error, int32_t cleanup) {
    if (cleanup == 0)
        std::fprintf(stderr, "[xess-fg] initialization stage %s failed (%d)\n", stage, error);
    else
        std::fprintf(stderr, "[xess-fg] initialization failed (%d), release failed (%d); native owner retained\n",
            error, cleanup);
}

static int32_t ReadCompletion(VulkanStoryXessFg* value, uint64_t* completed) {
    if (!value->completionFence) return static_cast<int32_t>(E_UNEXPECTED);
    *completed = value->completionFence->GetCompletedValue();
    return *completed == UINT64_MAX ? static_cast<int32_t>(DXGI_ERROR_DEVICE_REMOVED) : 0;
}

static int32_t DestroyFg(VulkanStoryXessFg* value) {
    if (!value->fg) return 0;
    if (!value->destroyFg) return -1;
    int code = static_cast<int>(value->destroyFg(value->fg));
    if (code >= 0) value->fg = nullptr;
    return code;
}

static int32_t DestroyXell(VulkanStoryXessFg* value) {
    if (!value->xell) return 0;
    if (!value->destroyXell) return -1;
    int code = static_cast<int>(value->destroyXell(value->xell));
    if (code >= 0) value->xell = nullptr;
    return code;
}

static bool IsStarted(const VulkanStoryXessFg* value) {
    return rsf_core_xess_bridge_started(value->session) != 0;
}

extern "C" __declspec(dllexport) int VulkanStoryXessFgWaitIdle(VulkanStoryXessFg* value) {
    return value ? rsf_core_xess_bridge_wait_idle(value->session) : -1;
}

extern "C" __declspec(dllexport) const char* VulkanStoryXessFgLastCreateStage() {
    return rsf_core_xess_bridge_create_stage();
}

// The host stops admission, then drains its Vulkan users before retiring the
// SDK consumers. Shared input resources stay alive until preparation succeeds.
extern "C" __declspec(dllexport) int VulkanStoryXessFgPrepareDestroy(VulkanStoryXessFg* value) {
    return value ? rsf_core_xess_bridge_prepare_destroy(value->session) : 0;
}

extern "C" __declspec(dllexport) int VulkanStoryXessFgDestroyChecked(VulkanStoryXessFg* value) {
    return value ? rsf_core_xess_bridge_destroy(value->session) : 0;
}

extern "C" __declspec(dllexport) void VulkanStoryXessFgDestroy(VulkanStoryXessFg* value) {
    VulkanStoryXessFgDestroyChecked(value);
}

static int CreateContext(uint64_t adapterLuid, const wchar_t* directory, VulkanStoryXessFg** output);

extern "C" __declspec(dllexport) int VulkanStoryXessFgCreate(uint64_t adapterLuid,
    VulkanStoryXessFg** output) {
    return CreateContext(adapterLuid, nullptr, output);
}

extern "C" __declspec(dllexport) int32_t RsfXessBridgeCreate(
    const rsf_xess_bridge_create* setup, rsf_xess_bridge** output) {
    if (!output) return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    *output = nullptr;
    if (!setup) return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    int32_t check = rsf_xess_bridge_check(setup->struct_size, setup->abi_version, sizeof(*setup));
    if (check) return check;
    static_assert(sizeof(wchar_t) == sizeof(uint16_t));
    const auto* directory = reinterpret_cast<const wchar_t*>(setup->runtime_directory);
    // Accept drive-rooted or UNC absolute paths, never a DLL search-path input.
    if (!directory || !directory[0] ||
        !((directory[0] && directory[1] == L':' &&
           (directory[2] == L'\\' || directory[2] == L'/')) ||
          (directory[0] == L'\\' && directory[1] == L'\\')))
        return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    return CreateContext(setup->adapter_luid, directory, output);
}

extern "C" __declspec(dllexport) uint64_t RsfXessBridgeAdapterLuid(const rsf_xess_bridge* value) {
    DXGI_ADAPTER_DESC1 desc{};
    if (!value || !value->adapter || FAILED(value->adapter->GetDesc1(&desc))) return 0;
    uint64_t luid = 0;
    static_assert(sizeof(luid) == sizeof(desc.AdapterLuid));
    std::memcpy(&luid, &desc.AdapterLuid, sizeof(luid));
    return luid;
}

/// @brief Queries the selected Vulkan device LUID and creates its matching native presentation context.
/// @details The Vulkan physical device is borrowed and must expose a valid Windows LUID.
/// @return Native creation result, or a negative argument/Vulkan-query availability code.
extern "C" __declspec(dllexport) int VulkanStoryXessFgCreateFromVulkan(
    VkPhysicalDevice physical, VulkanStoryXessFg** output) {
    rsf_core_xess_bridge_note_vulkan_lookup();
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

// The borrowed HWND must be free of other swapchain owners. Per-frame VSync
// is applied by the native presentation path.
static int32_t InitSwapchain(VulkanStoryXessFg* value, void* window, uint32_t width,
    uint32_t height, uint32_t backbufferHudless, uint32_t invertedDepth) {
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
    params.uiMode = backbufferHudless ? XEFG_SWAPCHAIN_UI_MODE_BACKBUFFER_HUDLESS :
        XEFG_SWAPCHAIN_UI_MODE_HUDLESS_UITEXTURE;
    params.initFlags = invertedDepth ? XEFG_SWAPCHAIN_INIT_FLAG_INVERTED_DEPTH : 0;
    return static_cast<int32_t>(value->initSwapchain(value->fg, static_cast<HWND>(window),
        &desc, nullptr, value->queue, value->factory, &params));
}

static int32_t QueryLimit(VulkanStoryXessFg* value, uint32_t* maximum) {
    xefg_swapchain_properties_t properties{};
    int code = static_cast<int>(value->getProperties(value->fg, &properties));
    if (code >= 0) *maximum = properties.maxSupportedInterpolations;
    return code;
}

static int32_t GetSwapchain(VulkanStoryXessFg* value) {
    int code = static_cast<int>(value->getSwapchain(value->fg,
        __uuidof(IDXGISwapChain4), reinterpret_cast<void**>(&value->swapchain)));
    return code >= 0 && !value->swapchain ? -2 : code;
}

static int32_t CreateRecorders(VulkanStoryXessFg* value) {
    HRESULT result;
    for (auto*& allocator : value->allocators) {
        result = value->device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,
            IID_PPV_ARGS(&allocator));
        if (FAILED(result)) return static_cast<int>(result);
    }
    result = value->device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT,
        value->allocators[0], nullptr, IID_PPV_ARGS(&value->commandList));
    if (FAILED(result)) return static_cast<int>(result);
    return static_cast<int>(value->commandList->Close());
}

static int32_t StartCapture(VulkanStoryXessFg* value, void* module) noexcept try {
    return value->capture.initialize(static_cast<HMODULE>(module), value->fg) ? 0 : -7011;
} catch (...) {
    return -7011;
}

static int32_t SetGenerated(VulkanStoryXessFg* value, uint32_t count) {
    int code = static_cast<int>(value->setInterpolations(value->fg, count));
    if (code >= 0) value->activeInterpolations = count;
    return code;
}

extern "C" __declspec(dllexport) int32_t RsfXessBridgeCaptureSource(rsf_xess_bridge* value,
    const rsf_frame_record* record) {
    if (!value || !record || record->struct_size < sizeof(*record) || record->camera.struct_size < sizeof(record->camera) ||
        !record->frame_id || record->frame_id > UINT32_MAX || !record->session_id) return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    if (record->abi_version != RSF_GAME_FRAME_ABI_VERSION || record->camera.abi_version != RSF_GAME_CAMERA_ABI_VERSION)
        return RSF_XESS_BRIDGE_ABI_MISMATCH;
    return value->capture.source(*record, value->activeInterpolations) ? 0 : -7012;
}

extern "C" __declspec(dllexport) uint64_t RsfXessBridgeCapturedCallbacks(const rsf_xess_bridge* value) {
    return value ? value->capture.observed() : 0;
}

extern "C" __declspec(dllexport) int VulkanStoryXessFgStart(VulkanStoryXessFg* value,
    HWND window, uint32_t width, uint32_t height, uint32_t vsync) {
    (void)vsync;
    return value ? rsf_core_xess_bridge_start(value->session, window, width, height, 0, 0,
        XEFG_SWAPCHAIN_RESULT_ERROR_UNSUPPORTED) : -1;
}

extern "C" __declspec(dllexport) int32_t RsfXessBridgeStart(rsf_xess_bridge* value,
    const rsf_xess_bridge_start* setup) {
    if (!value || !setup) return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    int32_t check = rsf_xess_bridge_check(setup->struct_size, setup->abi_version, sizeof(*setup));
    if (check) return check;
    if (!setup->generated_count || setup->inverted_depth > 1) return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    return rsf_core_xess_bridge_start(value->session, setup->window, setup->width, setup->height,
        setup->inverted_depth, setup->generated_count, XEFG_SWAPCHAIN_RESULT_ERROR_UNSUPPORTED);
}

extern "C" __declspec(dllexport) uint32_t RsfXessBridgeMaxGenerated(const rsf_xess_bridge* value) {
    return value ? rsf_core_xess_bridge_max_generated(value->session) : 0;
}

extern "C" __declspec(dllexport) int32_t RsfXessBridgeSetGenerated(rsf_xess_bridge* value, uint32_t count) {
    if (!value || !IsStarted(value) || !count) return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    if (count > RsfXessBridgeMaxGenerated(value)) return XEFG_SWAPCHAIN_RESULT_ERROR_UNSUPPORTED;
    if (count == value->activeInterpolations) return 0;
    return SetGenerated(value, count);
}

/// @brief Forwards frame-generation enablement for an already started native presenter.
/// @return SDK result, or -1 without a started FG context.
extern "C" __declspec(dllexport) int VulkanStoryXessFgSetEnabled(VulkanStoryXessFg* value,
    uint32_t enabled) {
    return value && value->fg && IsStarted(value) ?
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
    if (!value || !value->fg || !IsStarted(value) || !effective || !maximum) return -1;
    const auto max = RsfXessBridgeMaxGenerated(value);
    if (!max) return -1;
    *maximum = max;
    *effective = requested < 1 ? 1 : requested > max ? max : requested;
    if (*effective == value->activeInterpolations) return 0;
    return SetGenerated(value, *effective);
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

static int32_t WaitShared(VulkanStoryXessFg* value, uint64_t target) {
    return static_cast<int32_t>(value->queue->Wait(value->sharedFence, target));
}

static int32_t SignalDone(VulkanStoryXessFg* value, uint64_t target) {
    return static_cast<int32_t>(value->queue->Signal(value->sharedFence, target));
}

/// @brief Queues a DX12 GPU wait for the borrowed shared-fence value.
/// @details This returns after enqueuing the wait; it does not synchronously wait on the CPU.
/// @return Zero when enqueued, otherwise local/HRESULT failure.
extern "C" __declspec(dllexport) int VulkanStoryXessFgWaitSharedFence(
    VulkanStoryXessFg* value, uint64_t fenceValue) {
    if (!value || !value->queue || !value->sharedFence || !fenceValue) return -1;
    const int32_t code = WaitShared(value, fenceValue);
    return code < 0 ? code : 0;
}

/// @brief Queues a DX12 shared-fence signal behind prior queue work.
/// @details The caller must reserve ordered values and must not equate a successful enqueue with immediate GPU completion.
/// @return Zero when enqueued, otherwise local/HRESULT failure.
extern "C" __declspec(dllexport) int VulkanStoryXessFgSignalSharedFence(
    VulkanStoryXessFg* value, uint64_t fenceValue) {
    if (!value || !value->queue || !value->sharedFence || !fenceValue) return -1;
    const int32_t code = SignalDone(value, fenceValue);
    return code < 0 ? code : 0;
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
    constexpr uint32_t queryCount = RSF_XESS_BRIDGE_ALLOCATOR_COUNT * 3;
    D3D12_QUERY_HEAP_DESC heap{};
    heap.Type = D3D12_QUERY_HEAP_TYPE_TIMESTAMP;
    heap.Count = queryCount;
    if (FAILED(value->device->CreateQueryHeap(&heap, IID_PPV_ARGS(&value->queryHeap)))) return false;
    D3D12_HEAP_PROPERTIES readbackHeap{};
    readbackHeap.Type = D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC buffer{};
    buffer.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    buffer.Width = queryCount * sizeof(uint64_t);
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

static int32_t CheckPresentResources(const VulkanStoryXessFrame* frame, uint32_t width,
    uint32_t height, uint32_t backbufferHudless) {
    D3D12_RESOURCE_DESC color = frame->color->GetDesc();
    D3D12_RESOURCE_DESC hudless = frame->hudless->GetDesc();
    D3D12_RESOURCE_DESC ui = frame->ui ? frame->ui->GetDesc() : D3D12_RESOURCE_DESC{};
    D3D12_RESOURCE_DESC motion = frame->motion->GetDesc();
    D3D12_RESOURCE_DESC depth = frame->depth->GetDesc();
    if (color.Width != width || color.Height != height ||
        color.Format != DXGI_FORMAT_B8G8R8A8_UNORM ||
        hudless.Width != width || hudless.Height != height ||
        hudless.Format != color.Format ||
        (!backbufferHudless && (ui.Width != width ||
        ui.Height != height || ui.Format != color.Format)) ||
        motion.Width != depth.Width || motion.Height != depth.Height ||
        motion.Format != DXGI_FORMAT_R16G16_FLOAT ||
        (backbufferHudless ? depth.Format != DXGI_FORMAT_R32_FLOAT :
            depth.Format != DXGI_FORMAT_D32_FLOAT)) return -2;
    return 0;
}

static int32_t ResetPresentCommands(VulkanStoryXessFg* value, uint32_t slot) {
    const HRESULT result = value->allocators[slot]->Reset();
    if (FAILED(result)) return static_cast<int32_t>(result);
    return static_cast<int32_t>(value->commandList->Reset(value->allocators[slot], nullptr));
}

// The copy, input tags and state restores borrow the same typed command list
// and backbuffer reference. On failure, close before returning to the session.
static int32_t RecordPresentInputs(VulkanStoryXessFg* value, const VulkanStoryXessFrame* frame) {
    std::array<ID3D12Resource*, 5> inputs = {
        frame->color, frame->depth, frame->motion, frame->hudless, frame->ui
    };
    // Real-frame fallback can use final color as HUD-less. Transition an aliased
    // resource once while still tagging it for the requested SDK input role.
    for (size_t i = 0; i < inputs.size(); ++i)
        for (size_t j = 0; j < i; ++j)
            if (inputs[i] && inputs[i] == inputs[j]) { inputs[i] = nullptr; break; }
    for (ID3D12Resource* input : inputs) {
        if (!input) continue;
        Transition(value->commandList, input, D3D12_RESOURCE_STATE_COMMON,
            D3D12_RESOURCE_STATE_COPY_SOURCE);
    }
    ID3D12Resource* backbuffer = nullptr;
    uint32_t backbufferIndex = value->swapchain->GetCurrentBackBufferIndex();
    const HRESULT result = value->swapchain->GetBuffer(backbufferIndex, IID_PPV_ARGS(&backbuffer));
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
        if (!tag.resource) continue;
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
    for (ID3D12Resource* input : inputs) {
        if (!input) continue;
        Transition(value->commandList, input, D3D12_RESOURCE_STATE_COPY_SOURCE,
            D3D12_RESOURCE_STATE_COMMON);
    }
    Transition(value->commandList, backbuffer, D3D12_RESOURCE_STATE_COPY_DEST,
        D3D12_RESOURCE_STATE_PRESENT);
    Release(backbuffer);
    return 0;
}

static int32_t TagPresentConstants(VulkanStoryXessFg* value, const VulkanStoryXessFrame* frame) {
    xefg_swapchain_frame_constant_data_t constants{};
    std::memcpy(constants.viewMatrix, frame->viewMatrix, sizeof(constants.viewMatrix));
    std::memcpy(constants.projectionMatrix, frame->projectionMatrix,
        sizeof(constants.projectionMatrix));
    constants.jitterOffsetX = frame->jitterX;
    constants.jitterOffsetY = frame->jitterY;
    constants.motionVectorScaleX = frame->motionScaleX;
    constants.motionVectorScaleY = frame->motionScaleY;
    constants.resetHistory = frame->reset;
    return static_cast<int32_t>(value->tagConstants(value->fg, frame->frameId, &constants));
}

static void ExecutePresentCommands(VulkanStoryXessFg* value) {
    ID3D12CommandList* lists[] = {value->commandList};
    value->queue->ExecuteCommandLists(1, lists);
}

static int32_t PresentSwapchain(VulkanStoryXessFg* value, uint32_t vsync) {
    const uint32_t flags = !vsync && value->allowTearing ? DXGI_PRESENT_ALLOW_TEARING : 0;
    return static_cast<int32_t>(value->swapchain->Present(vsync ? 1 : 0, flags));
}

static int32_t ReadPresentStatus(VulkanStoryXessFg* value, rsf_xess_bridge_present_result* output) {
    xefg_swapchain_present_status_t status{};
    const int32_t code = static_cast<int32_t>(value->presentStatus(value->fg, &status));
    if (code >= 0) {
        output->frames_presented = status.framesPresented;
        output->fg_result = static_cast<int32_t>(status.frameGenResult);
        output->fg_enabled = status.isFrameGenEnabled;
    }
    return code;
}

static uint32_t BeginGpuTiming(VulkanStoryXessFg* value, uint32_t slot) {
    if (!EnsureGpuTiming(value)) return 0;
    HarvestGpuTiming(value, slot);
    return 1;
}

static void TimestampCopies(VulkanStoryXessFg* value, uint32_t slot, uint32_t index) {
    value->commandList->EndQuery(value->queryHeap, D3D12_QUERY_TYPE_TIMESTAMP, slot * 3 + index);
}

static void FinishGpuTiming(VulkanStoryXessFg* value, uint32_t slot) {
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

static constexpr rsf_xess_bridge_platform Platform{
    .struct_size = sizeof(rsf_xess_bridge_platform),
    .abi_version = RSF_XESS_BRIDGE_PLATFORM_ABI_VERSION,
    .allocate = Allocate,
    .release_native = ReleaseNative,
    .load_runtime = LoadRuntime,
    .unload_module = [](void* module) { FreeLibrary(static_cast<HMODULE>(module)); },
    .create_factory = CreateFactory,
    .find_adapter = FindAdapter,
    .create_device = CreateDevice,
    .create_queue = CreateQueue,
    .create_xell = CreateXell,
    .create_fg = CreateFg,
    .set_latency = [](VulkanStoryXessFg* value) -> int32_t {
        return value->setLatency(value->fg, value->xell);
    },
    .set_default_sleep_mode = [](VulkanStoryXessFg* value) -> int32_t {
        xell_sleep_params_t mode{};
        mode.bLowLatencyMode = 1;
        return value->setSleepMode(value->xell, &mode);
    },
    .log_create_error = LogCreateError,
    .init_swapchain = InitSwapchain,
    .query_limit = QueryLimit,
    .get_swapchain = GetSwapchain,
    .enable_ui = [](VulkanStoryXessFg* value) -> int32_t {
        return value->setUi(value->fg, XEFG_SWAPCHAIN_UI_COMPOSITION_STATE_ENABLED);
    },
    .create_completion_fence = [](VulkanStoryXessFg* value) -> int32_t {
        return value->device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&value->completionFence));
    },
    .open_event = []() -> void* { return CreateEventW(nullptr, FALSE, FALSE, nullptr); },
    .create_recorders = CreateRecorders,
    .start_capture = StartCapture,
    .set_generated = SetGenerated,
    .read_completion = ReadCompletion,
    .arm_completion = [](VulkanStoryXessFg* value, uint64_t target, void* event) -> int32_t {
        return value->completionFence->SetEventOnCompletion(target, event);
    },
    .wait_event = [](void* event, uint32_t timeout) -> int32_t {
        return WaitForSingleObject(event, timeout) == WAIT_OBJECT_0 ? 0 : -3;
    },
    .disable_fg = [](VulkanStoryXessFg* value) -> int32_t {
        return value->fg && value->setEnabled ? static_cast<int>(value->setEnabled(value->fg, 0)) : -1;
    },
    .flush_capture = [](VulkanStoryXessFg* value) -> int32_t { return value->capture.flush() ? 0 : -7010; },
    .release_swapchain = [](VulkanStoryXessFg* value) { Release(value->swapchain); },
    .destroy_fg = DestroyFg,
    .destroy_xell = DestroyXell,
    .has_shared_queue = [](const VulkanStoryXessFg* value) -> uint32_t {
        return value->queue && value->sharedFence;
    },
    .check_resources = CheckPresentResources,
    .reset_commands = ResetPresentCommands,
    .record_inputs = RecordPresentInputs,
    .close_commands = [](VulkanStoryXessFg* value) -> int32_t { return value->commandList->Close(); },
    .tag_constants = TagPresentConstants,
    .wait_shared = WaitShared,
    .execute = ExecutePresentCommands,
    .set_present_id = [](VulkanStoryXessFg* value, uint32_t frameId) -> int32_t {
        return value->setPresentId(value->fg, frameId);
    },
    .present = PresentSwapchain,
    .signal_done = SignalDone,
    .signal_completion = [](VulkanStoryXessFg* value, uint64_t target) -> int32_t {
        return value->queue->Signal(value->completionFence, target);
    },
    .read_present_status = ReadPresentStatus,
    .timing_enabled = []() -> uint32_t { return timing.enabled; },
    .timing_now = []() -> int64_t { return timing.Now(); },
    .timing_add = [](uint32_t phase, int64_t start, int64_t end) {
        timing.Add(static_cast<TimingPhase>(phase), start, end);
    },
    .timing_end_present = []() { timing.EndPresent(); },
    .begin_gpu_timing = BeginGpuTiming,
    .timestamp_copies = TimestampCopies,
    .mark_present_enter = [](VulkanStoryXessFg* value, uint32_t slot) {
        value->presentEnter[slot] = timing.Now();
    },
    .finish_gpu_timing = FinishGpuTiming,
    .serialize_enabled = []() -> uint32_t {
        static const bool enabled = std::getenv("VULKANSTORY_XESS_FG_SERIALIZE") != nullptr;
        return enabled;
    },
};

static int CreateContext(uint64_t adapterLuid, const wchar_t* directory, VulkanStoryXessFg** output) {
    return rsf_core_xess_bridge_create(&Platform, adapterLuid,
        reinterpret_cast<const uint16_t*>(directory), output);
}

/// @brief Presents a prepared frame and preserves the ordered shared-fence handoff on recoverable failure.
/// @details The session queues readiness, done and local completion in order if Vulkan is still waiting for handoff.
/// @param value Borrowed started native presenter; host calls are serialized.
/// @param frame Borrowed matching resources/constants with ordered ready/done values.
/// @param framesPresented Required native status output; zeroed before presentation.
/// @param frameGenResult Required interpolation-result output, separate from this function result.
/// @param frameGenEnabled Required native FG enablement-status output.
/// @return Zero on successful presentation/status query; local/HRESULT/SDK error otherwise.
extern "C" __declspec(dllexport) int VulkanStoryXessFgPresent(VulkanStoryXessFg* value,
    const VulkanStoryXessFrame* frame, uint32_t* framesPresented,
    int* frameGenResult, uint32_t* frameGenEnabled) {
    return rsf_core_xess_bridge_present(value ? value->session : nullptr,
        frame, framesPresented, frameGenResult, frameGenEnabled);
}

extern "C" __declspec(dllexport) int32_t RsfXessBridgePresent(rsf_xess_bridge* value,
    const rsf_xess_bridge_frame* frame, rsf_xess_bridge_present_status* status) {
    return rsf_core_xess_bridge_present_integrated(value ? value->session : nullptr, frame, status);
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
