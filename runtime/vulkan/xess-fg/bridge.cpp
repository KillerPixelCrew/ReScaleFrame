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
#include <iterator>
#include <new>
#include <string>
#include <cstdio>
#include <cstdlib>
#include "xess_fg/xefg_swapchain_d3d12.h"
#include "xell/xell_d3d12.h"
#define RSF_XESS_BRIDGE_BUILD
#include "session_abi.h"
#include "diagnostic-capture.h"

static_assert(XEFG_SWAPCHAIN_USE_MAX_SUPPORTED_INTERPOLATED_FRAMES == UINT32_MAX);
static_assert(sizeof(xefg_swapchain_d3d12_init_params_t) == 72 &&
    offsetof(xefg_swapchain_d3d12_init_params_t, uiMode) == 64);
static_assert(sizeof(xefg_swapchain_properties_t) == 40 &&
    offsetof(xefg_swapchain_properties_t, maxSupportedInterpolations) == 32);
static_assert(sizeof(xefg_swapchain_frame_constant_data_t) == 152 &&
    offsetof(xefg_swapchain_frame_constant_data_t, resetHistory) == 144);
static_assert(sizeof(xefg_swapchain_present_status_t) == 12 && sizeof(xell_sleep_params_t) == 8);

/// @brief Releases a local COM reference and clears its pointer without GPU waiting.
template<typename T> static void Release(T*& value) {
    if (value) { value->Release(); value = nullptr; }
}

// Native clock and stderr leaves for the Rust timing owner. The diagnostic
// setting is sampled at bridge load, and does not affect pacing or Present.
namespace {
constexpr uint32_t kSleep = 5;
const int64_t timing_frequency = [] {
    LARGE_INTEGER frequency{};
    return QueryPerformanceFrequency(&frequency) ? frequency.QuadPart : 0;
}();
const bool timing_enabled = timing_frequency > 0 && std::getenv("VULKANSTORY_XESS_FG_TIMING") != nullptr;

int64_t TimingNow() {
    LARGE_INTEGER ticks{};
    return QueryPerformanceCounter(&ticks) ? ticks.QuadPart : 0;
}

void WriteTimingReport(const rsf_xess_timing_report* report) {
    constexpr const char* names[] = {
        "allocator_wait", "record", "submit", "dxgi_present", "retire", "xell_sleep"
    };
    std::fprintf(stderr, "[xess-fg timing] presents=%u", report->presents);
    for (size_t i = 0; i < std::size(names); ++i)
        std::fprintf(stderr, " %s_mean_ms=%.3f %s_max_ms=%.3f", names[i], report->phase_mean[i],
            names[i], report->phase_max[i]);
    if (report->gpu_samples)
        std::fprintf(stderr, " dx12_gpu_mean_ms=%.3f dx12_gpu_max_ms=%.3f gpu_start_after_present_enter_ms=%.3f"
            " present_exit_after_gpu_end_ms=%.3f dx12_our_copies_ms=%.3f", report->gpu_mean, report->gpu_max,
            report->gpu_start_after_present_enter, report->present_exit_after_gpu_end, report->copies_mean);
    std::fprintf(stderr, " sequence_ms=");
    for (uint32_t i = 0; i < report->sequence_count; ++i)
        std::fprintf(stderr, i ? ",%.1f" : "%.1f", report->sequence[i]);
    std::fputc('\n', stderr);
    std::fflush(stderr);
}
}

// Typed storage for the Rust session and its presentation leaves. The public
// handle may remain live after failed retirement; only the session frees it.
struct VulkanStoryXessFg {
    rsf_xess_bridge_session* session = nullptr;
    const rsf_xess_timing* timing = nullptr;
    // Borrowed from the session; the SDK diagnostic callback user.
    rsf_xess_capture_owner* capture = nullptr;
    rsf_xess_capture::SetDiagnostics setDiagnostics = nullptr;
    xefg_swapchain_handle_t diagnosticContext = nullptr;
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

static_assert(sizeof(VulkanStoryXessFrame) == 216 && alignof(VulkanStoryXessFrame) == 8);
static_assert(offsetof(VulkanStoryXessFrame, color) == 16);
static_assert(offsetof(VulkanStoryXessFrame, view) == 56);
static_assert(offsetof(VulkanStoryXessFrame, ready) == 200);
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
static int32_t LoadRuntime(const uint16_t* directory,
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
    return 0;
} catch (...) {
    return -3;
}

static void BindRuntime(VulkanStoryXessFg* value, const rsf_xess_bridge_sdk_entries* entries) {
    value->createXell = reinterpret_cast<decltype(value->createXell)>((*entries)[0]);
    value->destroyXell = reinterpret_cast<decltype(value->destroyXell)>((*entries)[1]);
    value->setSleepMode = reinterpret_cast<decltype(value->setSleepMode)>((*entries)[2]);
    value->sleep = reinterpret_cast<decltype(value->sleep)>((*entries)[3]);
    value->marker = reinterpret_cast<decltype(value->marker)>((*entries)[4]);
    value->createFg = reinterpret_cast<decltype(value->createFg)>((*entries)[5]);
    value->destroyFg = reinterpret_cast<decltype(value->destroyFg)>((*entries)[6]);
    value->setLatency = reinterpret_cast<decltype(value->setLatency)>((*entries)[7]);
    value->initSwapchain = reinterpret_cast<decltype(value->initSwapchain)>((*entries)[8]);
    value->getSwapchain = reinterpret_cast<decltype(value->getSwapchain)>((*entries)[9]);
    value->setEnabled = reinterpret_cast<decltype(value->setEnabled)>((*entries)[10]);
    value->setPresentId = reinterpret_cast<decltype(value->setPresentId)>((*entries)[11]);
    value->setInterpolations = reinterpret_cast<decltype(value->setInterpolations)>((*entries)[12]);
    value->getProperties = reinterpret_cast<decltype(value->getProperties)>((*entries)[13]);
    value->setUi = reinterpret_cast<decltype(value->setUi)>((*entries)[14]);
    value->tagResource = reinterpret_cast<decltype(value->tagResource)>((*entries)[15]);
    value->tagConstants = reinterpret_cast<decltype(value->tagConstants)>((*entries)[16]);
    value->presentStatus = reinterpret_cast<decltype(value->presentStatus)>((*entries)[17]);
}

static VulkanStoryXessFg* Allocate(rsf_xess_bridge_session* session, const rsf_xess_timing* timing) noexcept try {
    auto* value = new (std::nothrow) VulkanStoryXessFg();
    if (value) {
        value->session = session;
        value->timing = timing;
    }
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
    uint32_t height, const xefg_swapchain_d3d12_init_params_t* params) {
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
    return static_cast<int32_t>(value->initSwapchain(value->fg, static_cast<HWND>(window),
        &desc, nullptr, value->queue, value->factory, params));
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

// The session admits the exact SDK binary before resolving its private setter.
static void BindDiagnostics(VulkanStoryXessFg* value, void* address) {
    value->setDiagnostics = reinterpret_cast<rsf_xess_capture::SetDiagnostics>(address);
}

static int32_t RegisterDiagnostics(VulkanStoryXessFg* value) {
    rsf_xess_capture::DiagnosticsV2 diagnostics;
    diagnostics.user = value->capture;
    diagnostics.input = rsf_xess_capture::Input;
    diagnostics.before_outputs = rsf_xess_capture::BeforeOutputs;
    diagnostics.output = rsf_xess_capture::Output;
    const int result = value->setDiagnostics(value->fg, &diagnostics);
    if (result >= 0) value->diagnosticContext = value->fg;
    return result;
}

static int32_t UnregisterDiagnostics(VulkanStoryXessFg* value) {
    if (!value->diagnosticContext || !value->setDiagnostics) return 0;
    const int result = value->setDiagnostics(value->diagnosticContext, nullptr);
    if (result >= 0) value->diagnosticContext = nullptr;
    return result;
}

static int32_t SetGenerated(VulkanStoryXessFg* value, uint32_t count) {
    return static_cast<int>(value->setInterpolations(value->fg, count));
}

extern "C" __declspec(dllexport) int32_t RsfXessBridgeCaptureSource(rsf_xess_bridge* value,
    const rsf_frame_record* record) {
    return value ? rsf_core_xess_capture_source(value->capture, record,
                     rsf_core_xess_bridge_active_generated(value->session))
                 : RSF_XESS_BRIDGE_INVALID_ARGUMENT;
}

extern "C" __declspec(dllexport) uint64_t RsfXessBridgeCapturedCallbacks(const rsf_xess_bridge* value) {
    return value ? rsf_core_xess_capture_callbacks(value->capture) : 0;
}

extern "C" __declspec(dllexport) int VulkanStoryXessFgStart(VulkanStoryXessFg* value,
    HWND window, uint32_t width, uint32_t height, uint32_t vsync) {
    (void)vsync;
    return value ? rsf_core_xess_bridge_start(value->session, window, width, height, 0, 0,
        XEFG_SWAPCHAIN_RESULT_ERROR_UNSUPPORTED) : -1;
}

extern "C" __declspec(dllexport) int32_t RsfXessBridgeStart(rsf_xess_bridge* value,
    const rsf_xess_bridge_start* setup) {
    return rsf_core_xess_bridge_start_integrated(value ? value->session : nullptr, setup,
        XEFG_SWAPCHAIN_RESULT_ERROR_UNSUPPORTED);
}

extern "C" __declspec(dllexport) uint32_t RsfXessBridgeMaxGenerated(const rsf_xess_bridge* value) {
    return value ? rsf_core_xess_bridge_max_generated(value->session) : 0;
}

extern "C" __declspec(dllexport) int32_t RsfXessBridgeSetGenerated(rsf_xess_bridge* value, uint32_t count) {
    return value ? rsf_core_xess_bridge_set_generated(value->session, count, XEFG_SWAPCHAIN_RESULT_ERROR_UNSUPPORTED)
                 : RSF_XESS_BRIDGE_INVALID_ARGUMENT;
}

/// @brief Forwards frame-generation enablement for an already started native presenter.
/// @return SDK result, or -1 without a started FG context.
extern "C" __declspec(dllexport) int VulkanStoryXessFgSetEnabled(VulkanStoryXessFg* value,
    uint32_t enabled) {
    return value && value->fg && IsStarted(value) ?
        static_cast<int>(value->setEnabled(value->fg, enabled ? 1 : 0)) : -1;
}

/// @brief Clamps interpolation count to the started runtime's supported range and avoids redundant SDK updates.
/// @details Outputs describe the clamped request and maximum.
/// @param value Borrowed started native owner.
/// @param requested Requested generated frames per rendered frame; clamped to one through the SDK limit.
/// @param effective Required output for the clamped count.
/// @param maximum Required output for the native interpolation limit.
/// @return SDK result, zero for an unchanged count, or -1 for missing prerequisites.
extern "C" __declspec(dllexport) int VulkanStoryXessFgSetGeneratedFrames(
    VulkanStoryXessFg* value, uint32_t requested, uint32_t* effective, uint32_t* maximum) {
    return value && value->fg ?
        rsf_core_xess_bridge_set_generated_legacy(value->session, requested, effective, maximum) : -1;
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

#if !defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
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

#endif

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
/// @brief Reads the completed slot timestamps and the queue's clock calibration.
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
    const rsf_xess_timing_gpu_sample sample{gpuStart, gpuCopiesEnd, gpuEnd, value->gpuFrequency,
        gpuCalibration, cpuCalibration, value->presentEnter[slot], value->presentExit[slot]};
    rsf_core_xess_timing_gpu(value->timing, &sample, timing_frequency);
}
}

static_assert(DXGI_FORMAT_B8G8R8A8_UNORM == 87 && DXGI_FORMAT_R16G16_FLOAT == 34 &&
    DXGI_FORMAT_D32_FLOAT == 40 && DXGI_FORMAT_R32_FLOAT == 41);

#if !defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
static void DescribePresentResource(void* resource, rsf_xess_bridge_resource_desc* output) {
    const D3D12_RESOURCE_DESC desc = static_cast<ID3D12Resource*>(resource)->GetDesc();
    *output = {desc.Width, desc.Height, static_cast<uint32_t>(desc.Format)};
}

#endif

static int32_t ResetPresentCommands(VulkanStoryXessFg* value, uint32_t slot) {
    const HRESULT result = value->allocators[slot]->Reset();
    if (FAILED(result)) return static_cast<int32_t>(result);
    return static_cast<int32_t>(value->commandList->Reset(value->allocators[slot], nullptr));
}

static_assert(D3D12_RESOURCE_STATE_COMMON == 0 && D3D12_RESOURCE_STATE_PRESENT == 0 &&
    D3D12_RESOURCE_STATE_COPY_DEST == 0x400 && D3D12_RESOURCE_STATE_COPY_SOURCE == 0x800);

static void TransitionPresentResource(VulkanStoryXessFg* value, void* resource,
    uint32_t before, uint32_t after) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    rsf_xess_capture::rsf_core_xess_d3d12_present_transition(value->commandList, resource, before, after);
#else
    Transition(value->commandList, static_cast<ID3D12Resource*>(resource),
        static_cast<D3D12_RESOURCE_STATES>(before), static_cast<D3D12_RESOURCE_STATES>(after));
#endif
}

static int32_t AcquireBackbuffer(VulkanStoryXessFg* value, void** output) {
    ID3D12Resource* backbuffer = nullptr;
    const uint32_t backbufferIndex = value->swapchain->GetCurrentBackBufferIndex();
    const HRESULT result = value->swapchain->GetBuffer(backbufferIndex, IID_PPV_ARGS(&backbuffer));
    if (SUCCEEDED(result)) *output = backbuffer;
    return static_cast<int32_t>(result);
}

static void CopyPresentColor(VulkanStoryXessFg* value, void* backbuffer, void* color) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    rsf_xess_capture::rsf_core_xess_d3d12_present_copy(value->commandList, backbuffer, color);
#else
    value->commandList->CopyResource(static_cast<ID3D12Resource*>(backbuffer),
        static_cast<ID3D12Resource*>(color));
#endif
}

static void ExecutePresentCommands(VulkanStoryXessFg* value) {
    ID3D12CommandList* lists[] = {value->commandList};
    value->queue->ExecuteCommandLists(1, lists);
}

static int32_t PresentSwapchain(VulkanStoryXessFg* value, uint32_t vsync) {
    const uint32_t flags = !vsync && value->allowTearing ? DXGI_PRESENT_ALLOW_TEARING : 0;
    return static_cast<int32_t>(value->swapchain->Present(vsync ? 1 : 0, flags));
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
    value->presentExit[slot] = TimingNow();
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
    .set_sleep_mode = [](VulkanStoryXessFg* value, const xell_sleep_params_t* mode) -> int32_t {
        return value->setSleepMode(value->xell, mode);
    },
    .log_create_error = LogCreateError,
    .init_swapchain = InitSwapchain,
    .get_properties = [](VulkanStoryXessFg* value, xefg_swapchain_properties_t* properties) -> int32_t {
        return value->getProperties(value->fg, properties);
    },
    .get_swapchain = GetSwapchain,
    .enable_ui = [](VulkanStoryXessFg* value) -> int32_t {
        return value->setUi(value->fg, XEFG_SWAPCHAIN_UI_COMPOSITION_STATE_ENABLED);
    },
    .create_completion_fence = [](VulkanStoryXessFg* value) -> int32_t {
        return value->device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&value->completionFence));
    },
    .open_event = []() -> void* { return CreateEventW(nullptr, FALSE, FALSE, nullptr); },
    .create_recorders = CreateRecorders,
    .bind_diagnostics = BindDiagnostics,
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
    .unregister_diagnostics = UnregisterDiagnostics,
    .release_swapchain = [](VulkanStoryXessFg* value) { Release(value->swapchain); },
    .destroy_fg = DestroyFg,
    .destroy_xell = DestroyXell,
    .has_shared_queue = [](const VulkanStoryXessFg* value) -> uint32_t {
        return value->queue && value->sharedFence;
    },
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    .describe_present_resource = rsf_xess_capture::rsf_core_xess_d3d12_present_describe,
#else
    .describe_present_resource = DescribePresentResource,
#endif
    .reset_commands = ResetPresentCommands,
    .transition_present_resource = TransitionPresentResource,
    .acquire_backbuffer = AcquireBackbuffer,
    .copy_present_color = CopyPresentColor,
    .tag_present_resource = [](VulkanStoryXessFg* value, uint32_t frameId,
        const xefg_swapchain_d3d12_resource_data_t* data) -> int32_t {
        return value->tagResource(value->fg, value->commandList, frameId, data);
    },
    .release_backbuffer = [](void* backbuffer) { static_cast<ID3D12Resource*>(backbuffer)->Release(); },
    .close_commands = [](VulkanStoryXessFg* value) -> int32_t { return value->commandList->Close(); },
    .tag_constants = [](VulkanStoryXessFg* value, uint32_t frameId,
        const xefg_swapchain_frame_constant_data_t* data) -> int32_t {
        return value->tagConstants(value->fg, frameId, data);
    },
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
    .read_present_status = [](VulkanStoryXessFg* value, xefg_swapchain_present_status_t* status) -> int32_t {
        return value->presentStatus(value->fg, status);
    },
    .timing_enabled = []() -> uint32_t { return timing_enabled; },
    .timing_now = TimingNow,
    .timing_frequency = []() -> int64_t { return timing_frequency; },
    .timing_report = WriteTimingReport,
    .begin_gpu_timing = BeginGpuTiming,
    .timestamp_copies = TimestampCopies,
    .mark_present_enter = [](VulkanStoryXessFg* value, uint32_t slot) {
        value->presentEnter[slot] = TimingNow();
    },
    .finish_gpu_timing = FinishGpuTiming,
    .serialize_enabled = []() -> uint32_t {
        static const bool enabled = std::getenv("VULKANSTORY_XESS_FG_SERIALIZE") != nullptr;
        return enabled;
    },
    .attach_capture = [](VulkanStoryXessFg* value, rsf_xess_capture_owner* capture) { value->capture = capture; },
    .register_diagnostics = RegisterDiagnostics,
    .capture_digest = rsf_xess_capture::Digest,
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    .capture_describe = rsf_xess_capture::rsf_core_xess_d3d12_capture_describe,
    .capture_footprint = rsf_xess_capture::rsf_core_xess_d3d12_capture_footprint,
    .capture_readback = rsf_xess_capture::rsf_core_xess_d3d12_capture_readback,
    .capture_release = rsf_xess_capture::rsf_core_xess_d3d12_capture_release,
    .capture_copy = rsf_xess_capture::rsf_core_xess_d3d12_capture_copy,
    .capture_map = rsf_xess_capture::rsf_core_xess_d3d12_capture_map,
    .capture_unmap = rsf_xess_capture::rsf_core_xess_d3d12_capture_unmap,
#else
    .capture_describe = rsf_xess_capture::Describe,
    .capture_footprint = rsf_xess_capture::QueryFootprint,
    .capture_readback = rsf_xess_capture::CreateReadback,
    .capture_release = rsf_xess_capture::ReleaseObject,
    .capture_copy = rsf_xess_capture::RecordCopy,
    .capture_map = rsf_xess_capture::MapReadback,
    .capture_unmap = rsf_xess_capture::UnmapReadback,
#endif
    .lookup_export = [](void* module, const char* name) -> void* {
        return reinterpret_cast<void*>(GetProcAddress(static_cast<HMODULE>(module), name));
    },
    .bind_runtime = BindRuntime,
};

static int CreateContext(uint64_t adapterLuid, const wchar_t* directory, VulkanStoryXessFg** output) {
    return rsf_core_xess_bridge_create(&Platform, adapterLuid,
        reinterpret_cast<const uint16_t*>(directory), output);
}

extern "C" __declspec(dllexport) int32_t RsfXessBridgeCreate(
    const rsf_xess_bridge_create* setup, rsf_xess_bridge** output) {
    return rsf_core_xess_bridge_create_integrated(&Platform, setup, output);
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
    return rsf_core_xess_bridge_set_sleep_mode(value->session, minimumIntervalUs, enabled);
}

/// @brief Invokes XeLL sleep for the supplied frame before host input polling.
/// @details Optional timing measures CPU call duration only.
/// @return SDK result, or -1 without a XeLL context.
extern "C" __declspec(dllexport) int VulkanStoryXessFgSleep(VulkanStoryXessFg* value,
    uint32_t frameId) {
    if (!value || !value->xell) return -1;
    const int64_t start = timing_enabled ? TimingNow() : 0;
    int result = static_cast<int>(value->sleep(value->xell, frameId));
    if (timing_enabled) rsf_core_xess_timing_cpu(value->timing, kSleep, start, TimingNow(), timing_frequency);
    return result;
}

/// @brief Emits the supplied XeLL marker for the matching frame identity.
/// @return SDK result, or -1 without a XeLL context.
extern "C" __declspec(dllexport) int VulkanStoryXessFgMarker(VulkanStoryXessFg* value,
    uint32_t frameId, xell_latency_marker_type_t marker) {
    return value && value->xell ?
        static_cast<int>(value->marker(value->xell, frameId, marker)) : -1;
}
