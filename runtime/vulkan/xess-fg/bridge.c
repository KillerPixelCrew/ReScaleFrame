/* SPDX-License-Identifier: MIT */
/* XeSS-FG 3.0.2 DX12 presentation bridge leaves. The integrated host supplies
   the SDK directory and routes DXGI/D3D12 calls through the Runtime crate. The
   standalone bridge loads the DLLs beside itself and issues COM calls here.
   session.rs owns admission, startup and retirement order, Present records,
   timing aggregation and diagnostic capture. This file binds the SDK exports,
   Win32 loader, event and clock calls, and the remaining COM calls. */
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <vulkan/vulkan.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>
#include "xess_fg/xefg_swapchain_d3d12.h"
/* xell.h checks sizeof(_xell_latency_marker_type_t), which names the enum tag
   as a type. This typedef makes that check valid C; it adds no declaration. */
typedef enum _xell_latency_marker_type_t _xell_latency_marker_type_t;
#include "xell/xell_d3d12.h"
#define RSF_XESS_BRIDGE_BUILD
#include "session_abi.h"
#include "diagnostic-capture.h"
#include "dx12-runtime.h"
#include "../../common/include/rescaleframe/sha256_file.h"

#define RSF_BRIDGE_EXPORT __declspec(dllexport)

_Static_assert(XEFG_SWAPCHAIN_USE_MAX_SUPPORTED_INTERPOLATED_FRAMES == UINT32_MAX, "default interpolation sentinel");
_Static_assert(sizeof(xefg_swapchain_d3d12_init_params_t) == 72 &&
    offsetof(xefg_swapchain_d3d12_init_params_t, uiMode) == 64, "SDK init record");
_Static_assert(sizeof(xefg_swapchain_properties_t) == 40 &&
    offsetof(xefg_swapchain_properties_t, maxSupportedInterpolations) == 32, "SDK properties record");
_Static_assert(sizeof(xefg_swapchain_frame_constant_data_t) == 152 &&
    offsetof(xefg_swapchain_frame_constant_data_t, resetHistory) == 144, "SDK constants record");
_Static_assert(sizeof(xefg_swapchain_present_status_t) == 12 && sizeof(xell_sleep_params_t) == 8,
    "SDK status records");

/* Typed SDK entries, in the session's resolution order. */
typedef xell_result_t (*XellCreate)(ID3D12Device*, xell_context_handle_t*);
typedef xell_result_t (*XellDestroy)(xell_context_handle_t);
typedef xell_result_t (*XellSetSleepMode)(xell_context_handle_t, const xell_sleep_params_t*);
typedef xell_result_t (*XellSleep)(xell_context_handle_t, uint32_t);
typedef xell_result_t (*XellMarker)(xell_context_handle_t, uint32_t, xell_latency_marker_type_t);
typedef xefg_swapchain_result_t (*FgCreate)(ID3D12Device*, xefg_swapchain_handle_t*);
typedef xefg_swapchain_result_t (*FgDestroy)(xefg_swapchain_handle_t);
typedef xefg_swapchain_result_t (*FgSetLatency)(xefg_swapchain_handle_t, void*);
typedef xefg_swapchain_result_t (*FgInitSwapchain)(xefg_swapchain_handle_t, HWND, const DXGI_SWAP_CHAIN_DESC1*,
    const DXGI_SWAP_CHAIN_FULLSCREEN_DESC*, ID3D12CommandQueue*, IDXGIFactory2*,
    const xefg_swapchain_d3d12_init_params_t*);
typedef xefg_swapchain_result_t (*FgGetSwapchain)(xefg_swapchain_handle_t, REFIID, void**);
typedef xefg_swapchain_result_t (*FgSetEnabled)(xefg_swapchain_handle_t, uint32_t);
typedef xefg_swapchain_result_t (*FgSetPresentId)(xefg_swapchain_handle_t, uint32_t);
typedef xefg_swapchain_result_t (*FgSetInterpolations)(xefg_swapchain_handle_t, uint32_t);
typedef xefg_swapchain_result_t (*FgGetProperties)(xefg_swapchain_handle_t, xefg_swapchain_properties_t*);
typedef xefg_swapchain_result_t (*FgSetUi)(xefg_swapchain_handle_t, xefg_swapchain_ui_composition_state_t);
typedef xefg_swapchain_result_t (*FgTagResource)(xefg_swapchain_handle_t, ID3D12CommandList*, uint32_t,
    const xefg_swapchain_d3d12_resource_data_t*);
typedef xefg_swapchain_result_t (*FgTagConstants)(xefg_swapchain_handle_t, uint32_t,
    const xefg_swapchain_frame_constant_data_t*);
typedef xefg_swapchain_result_t (*FgPresentStatus)(xefg_swapchain_handle_t, xefg_swapchain_present_status_t*);
typedef xefg_swapchain_result_t (*FgSetLogging)(xefg_swapchain_handle_t, xefg_swapchain_logging_level_t,
    xefg_swapchain_app_log_callback_t, void*);

/* The selection is unevaluated, so these checks add no import references. */
#define RSF_SDK_TYPE(function, type) _Static_assert(_Generic(&function, type: 1, default: 0), #function)
RSF_SDK_TYPE(xellD3D12CreateContext, XellCreate);
RSF_SDK_TYPE(xellDestroyContext, XellDestroy);
RSF_SDK_TYPE(xellSetSleepMode, XellSetSleepMode);
RSF_SDK_TYPE(xellSleep, XellSleep);
RSF_SDK_TYPE(xellAddMarkerData, XellMarker);
RSF_SDK_TYPE(xefgSwapChainD3D12CreateContext, FgCreate);
RSF_SDK_TYPE(xefgSwapChainDestroy, FgDestroy);
RSF_SDK_TYPE(xefgSwapChainSetLatencyReduction, FgSetLatency);
RSF_SDK_TYPE(xefgSwapChainD3D12InitFromSwapChainDesc, FgInitSwapchain);
RSF_SDK_TYPE(xefgSwapChainD3D12GetSwapChainPtr, FgGetSwapchain);
RSF_SDK_TYPE(xefgSwapChainSetEnabled, FgSetEnabled);
RSF_SDK_TYPE(xefgSwapChainSetPresentId, FgSetPresentId);
RSF_SDK_TYPE(xefgSwapChainSetNumInterpolatedFrames, FgSetInterpolations);
RSF_SDK_TYPE(xefgSwapChainGetProperties, FgGetProperties);
RSF_SDK_TYPE(xefgSwapChainSetUiCompositionState, FgSetUi);
RSF_SDK_TYPE(xefgSwapChainD3D12TagFrameResource, FgTagResource);
RSF_SDK_TYPE(xefgSwapChainTagFrameConstants, FgTagConstants);
RSF_SDK_TYPE(xefgSwapChainGetLastPresentStatus, FgPresentStatus);
RSF_SDK_TYPE(xefgSwapChainSetLoggingCallback, FgSetLogging);

/* Releases a local COM reference and clears its slot without GPU waiting. */
static void ReleaseObject(void** slot) {
    if (*slot) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
        rsf_core_xess_d3d12_capture_release(*slot);
#else
        IUnknown* object = (IUnknown*)*slot;
        object->lpVtbl->Release(object);
#endif
        *slot = NULL;
    }
}
#define RSF_RELEASE(object) ReleaseObject((void**)&(object))

/* Native clock and stderr leaves for the Rust timing owner. The diagnostic
   setting is sampled once per process and does not affect pacing or Present. */
enum { kSleep = 5 };
static INIT_ONCE timing_once = INIT_ONCE_STATIC_INIT;
static int64_t timing_frequency_value;
static bool timing_enabled_value;

static BOOL CALLBACK InitTiming(PINIT_ONCE once, PVOID parameter, PVOID* context) {
    (void)once; (void)parameter; (void)context;
    LARGE_INTEGER frequency;
    timing_frequency_value = QueryPerformanceFrequency(&frequency) ? frequency.QuadPart : 0;
    timing_enabled_value = timing_frequency_value > 0 && getenv("VULKANSTORY_XESS_FG_TIMING") != NULL;
    return TRUE;
}

static int64_t TimingFrequency(void) {
    InitOnceExecuteOnce(&timing_once, InitTiming, NULL, NULL);
    return timing_frequency_value;
}

static uint32_t TimingEnabled(void) {
    InitOnceExecuteOnce(&timing_once, InitTiming, NULL, NULL);
    return timing_enabled_value;
}

static int64_t TimingNow(void) {
    LARGE_INTEGER ticks;
    return QueryPerformanceCounter(&ticks) ? ticks.QuadPart : 0;
}

static void WriteTimingReport(const rsf_xess_timing_report* report) {
    static const char* const names[] = {
        "allocator_wait", "record", "submit", "dxgi_present", "retire", "xell_sleep"
    };
    fprintf(stderr, "[xess-fg timing] presents=%u", report->presents);
    for (size_t i = 0; i < sizeof(names) / sizeof(names[0]); ++i)
        fprintf(stderr, " %s_mean_ms=%.3f %s_max_ms=%.3f", names[i], report->phase_mean[i],
            names[i], report->phase_max[i]);
    if (report->gpu_samples)
        fprintf(stderr, " dx12_gpu_mean_ms=%.3f dx12_gpu_max_ms=%.3f gpu_start_after_present_enter_ms=%.3f"
            " present_exit_after_gpu_end_ms=%.3f dx12_our_copies_ms=%.3f", report->gpu_mean, report->gpu_max,
            report->gpu_start_after_present_enter, report->present_exit_after_gpu_end, report->copies_mean);
    fprintf(stderr, " sequence_ms=");
    for (uint32_t i = 0; i < report->sequence_count; ++i)
        fprintf(stderr, i ? ",%.1f" : "%.1f", report->sequence[i]);
    fputc('\n', stderr);
    fflush(stderr);
}

static INIT_ONCE serialize_once = INIT_ONCE_STATIC_INIT;
static bool serialize_value;

static BOOL CALLBACK InitSerialize(PINIT_ONCE once, PVOID parameter, PVOID* context) {
    (void)once; (void)parameter; (void)context;
    serialize_value = getenv("VULKANSTORY_XESS_FG_SERIALIZE") != NULL;
    return TRUE;
}

static uint32_t SerializeEnabled(void) {
    InitOnceExecuteOnce(&serialize_once, InitSerialize, NULL, NULL);
    return serialize_value;
}

/* Typed storage for the Rust session and its presentation leaves. The public
   handle may remain live after failed retirement; only the session frees it. */
typedef struct rsf_vk_xess_fg rsf_vk_xess_fg;
struct rsf_vk_xess_fg {
    rsf_xess_bridge_session* session;
    const rsf_xess_timing* timing;
    /* Borrowed from the session; the SDK diagnostic callback user. */
    rsf_xess_capture_owner* capture;
    rsf_xess_set_diagnostics setDiagnostics;
    xefg_swapchain_handle_t diagnosticContext;
    IDXGIFactory4* factory;
    IDXGIAdapter1* adapter;
    ID3D12Device* device;
    ID3D12CommandQueue* queue;
    ID3D12Fence* sharedFence;
    IDXGISwapChain4* swapchain;
    ID3D12CommandAllocator* allocators[RSF_XESS_BRIDGE_ALLOCATOR_COUNT];
    ID3D12GraphicsCommandList* commandList;
    ID3D12Fence* completionFence;
    bool allowTearing;
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    rsf_xess_dx12_timestamps* timestamps;
#else
    /* Timing only (VULKANSTORY_XESS_FG_TIMING): three timestamps per allocator slot. */
    ID3D12QueryHeap* queryHeap;
    ID3D12Resource* queryReadback;
    ID3D12CommandAllocator* postAllocators[RSF_XESS_BRIDGE_ALLOCATOR_COUNT];
    ID3D12GraphicsCommandList* postList;
    uint64_t gpuFrequency;
    int64_t presentEnter[RSF_XESS_BRIDGE_ALLOCATOR_COUNT];
    int64_t presentExit[RSF_XESS_BRIDGE_ALLOCATOR_COUNT];
    bool slotTimed[RSF_XESS_BRIDGE_ALLOCATOR_COUNT];
#endif
    xell_context_handle_t xell;
    xefg_swapchain_handle_t fg;

    XellCreate createXell;
    XellDestroy destroyXell;
    XellSetSleepMode setSleepMode;
    XellSleep sleep;
    XellMarker marker;
    FgCreate createFg;
    FgDestroy destroyFg;
    FgSetLatency setLatency;
    FgInitSwapchain initSwapchain;
    FgGetSwapchain getSwapchain;
    FgSetEnabled setEnabled;
    FgSetPresentId setPresentId;
    FgSetInterpolations setInterpolations;
    FgGetProperties getProperties;
    FgSetUi setUi;
    FgTagResource tagResource;
    FgTagConstants tagConstants;
    FgPresentStatus presentStatus;
};

_Static_assert(sizeof(rsf_vk_xess_fg_frame) == 216 && _Alignof(rsf_vk_xess_fg_frame) == 8, "legacy frame");
_Static_assert(offsetof(rsf_vk_xess_fg_frame, color) == 16, "legacy frame color");
_Static_assert(offsetof(rsf_vk_xess_fg_frame, view) == 56, "legacy frame view");
_Static_assert(offsetof(rsf_vk_xess_fg_frame, ready) == 200, "legacy frame ready");
_Static_assert(sizeof(rsf_xess_bridge_frame) == 216 && _Alignof(rsf_xess_bridge_frame) == 8, "bridge frame");
_Static_assert(offsetof(rsf_xess_bridge_frame, color) == 24, "bridge frame color");
_Static_assert(offsetof(rsf_xess_bridge_frame, view_matrix) == 56, "bridge frame view");
_Static_assert(offsetof(rsf_xess_bridge_frame, ready_fence_value) == 200, "bridge frame ready");
_Static_assert(sizeof(rsf_xess_bridge_present_status) == 24, "bridge status");
_Static_assert(offsetof(rsf_xess_bridge_present_status, frames_presented) == 8, "bridge status count");
_Static_assert(offsetof(rsf_xess_bridge_present_status, reserved) == 20, "bridge status reserved");

/* Writes this bridge's directory, including the trailing separator, for sibling
   Intel runtime loading. Returns 0 when the module path is unavailable. */
static size_t ModuleDirectory(wchar_t (*path)[MAX_PATH]) {
    HMODULE own = NULL;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            (LPCWSTR)(uintptr_t)&ModuleDirectory, &own)) return 0;
    const DWORD length = GetModuleFileNameW(own, *path, MAX_PATH);
    if (!length || length == MAX_PATH) return 0;
    for (size_t i = length; i > 0; --i)
        if ((*path)[i - 1] == L'\\' || (*path)[i - 1] == L'/') return i;
    return 0;
}

static HMODULE LoadSibling(const wchar_t* base, size_t length, bool separator, const wchar_t* name, bool* failed) {
    const size_t nameLength = wcslen(name);
    wchar_t* path = (wchar_t*)malloc((length + 1 + nameLength + 1) * sizeof(wchar_t));
    if (!path) {
        *failed = true;
        return NULL;
    }
    memcpy(path, base, length * sizeof(wchar_t));
    size_t at = length;
    if (separator) path[at++] = L'\\';
    memcpy(path + at, name, (nameLength + 1) * sizeof(wchar_t));
    HMODULE module = LoadLibraryExW(path, NULL, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    free(path);
    return module;
}

/* Transfer each loaded module to the session, including on partial failure. */
static int32_t LoadRuntime(const uint16_t* directory, rsf_xess_bridge_modules* modules) {
    wchar_t own[MAX_PATH];
    const wchar_t* base = (const wchar_t*)directory;
    size_t length = base ? wcslen(base) : ModuleDirectory(&own);
    if (!base) base = own;
    if (!length) return -3;
    const bool separator = base[length - 1] != L'\\' && base[length - 1] != L'/';
    bool failed = false;
    HMODULE fgModule = LoadSibling(base, length, separator, L"libxess_fg.dll", &failed);
    if (failed) return -3;
    modules->fg = fgModule;
    HMODULE xellModule = LoadSibling(base, length, separator, L"libxell.dll", &failed);
    if (failed) return -3;
    modules->xell = xellModule;
    if (!fgModule || !xellModule) return -3;
    return 0;
}

static void UnloadModule(void* module) {
    FreeLibrary((HMODULE)module);
}

static void* LookupExport(void* module, const char* name) {
    return (void*)(uintptr_t)GetProcAddress((HMODULE)module, name);
}

static void BindRuntime(rsf_vk_xess_fg* value, const rsf_xess_bridge_sdk_entries* entries) {
    value->createXell = (XellCreate)(uintptr_t)(*entries)[0];
    value->destroyXell = (XellDestroy)(uintptr_t)(*entries)[1];
    value->setSleepMode = (XellSetSleepMode)(uintptr_t)(*entries)[2];
    value->sleep = (XellSleep)(uintptr_t)(*entries)[3];
    value->marker = (XellMarker)(uintptr_t)(*entries)[4];
    value->createFg = (FgCreate)(uintptr_t)(*entries)[5];
    value->destroyFg = (FgDestroy)(uintptr_t)(*entries)[6];
    value->setLatency = (FgSetLatency)(uintptr_t)(*entries)[7];
    value->initSwapchain = (FgInitSwapchain)(uintptr_t)(*entries)[8];
    value->getSwapchain = (FgGetSwapchain)(uintptr_t)(*entries)[9];
    value->setEnabled = (FgSetEnabled)(uintptr_t)(*entries)[10];
    value->setPresentId = (FgSetPresentId)(uintptr_t)(*entries)[11];
    value->setInterpolations = (FgSetInterpolations)(uintptr_t)(*entries)[12];
    value->getProperties = (FgGetProperties)(uintptr_t)(*entries)[13];
    value->setUi = (FgSetUi)(uintptr_t)(*entries)[14];
    value->tagResource = (FgTagResource)(uintptr_t)(*entries)[15];
    value->tagConstants = (FgTagConstants)(uintptr_t)(*entries)[16];
    value->presentStatus = (FgPresentStatus)(uintptr_t)(*entries)[17];
}

static rsf_vk_xess_fg* Allocate(rsf_xess_bridge_session* session, const rsf_xess_timing* timing) {
    rsf_vk_xess_fg* value = (rsf_vk_xess_fg*)calloc(1, sizeof(rsf_vk_xess_fg));
    if (value) {
        value->session = session;
        value->timing = timing;
    }
    return value;
}

static void ReleaseNative(rsf_vk_xess_fg* value) {
    RSF_RELEASE(value->commandList);
    for (size_t i = 0; i < RSF_XESS_BRIDGE_ALLOCATOR_COUNT; ++i) RSF_RELEASE(value->allocators[i]);
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    rsf_core_xess_dx12_timestamps_destroy(value->timestamps);
#else
    RSF_RELEASE(value->postList);
    for (size_t i = 0; i < RSF_XESS_BRIDGE_ALLOCATOR_COUNT; ++i) RSF_RELEASE(value->postAllocators[i]);
    RSF_RELEASE(value->queryReadback);
    RSF_RELEASE(value->queryHeap);
#endif
    RSF_RELEASE(value->completionFence);
    RSF_RELEASE(value->sharedFence);
    RSF_RELEASE(value->queue);
    RSF_RELEASE(value->device);
    RSF_RELEASE(value->adapter);
    RSF_RELEASE(value->factory);
    free(value);
}

#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
/* Stores a Runtime-created COM reference in a typed slot. */
#define RSF_BIND_COM(slot, call) do { void* bound_ = NULL; code = (call); (slot) = bound_; } while (0)
#endif

static int32_t CreateFactory(rsf_vk_xess_fg* value) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    int32_t code;
    RSF_BIND_COM(value->factory, rsf_core_xess_dx12_factory(&bound_));
    return code;
#else
    return FAILED(CreateDXGIFactory2(0, &IID_IDXGIFactory4, (void**)&value->factory)) ? -4 : 0;
#endif
}

static int32_t FindAdapter(rsf_vk_xess_fg* value, uint64_t adapterLuid) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    int32_t code;
    RSF_BIND_COM(value->adapter, rsf_core_xess_dx12_adapter(value->factory, adapterLuid, &bound_));
    return code;
#else
    LUID luid;
    luid.LowPart = (DWORD)adapterLuid;
    luid.HighPart = (LONG)(adapterLuid >> 32);
    return FAILED(value->factory->lpVtbl->EnumAdapterByLuid(value->factory, luid, &IID_IDXGIAdapter1,
        (void**)&value->adapter)) ? -5 : 0;
#endif
}

static int32_t CreateDevice(rsf_vk_xess_fg* value) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    int32_t code;
    RSF_BIND_COM(value->device, rsf_core_xess_dx12_device(value->adapter, &bound_));
    return code;
#else
    return FAILED(D3D12CreateDevice((IUnknown*)value->adapter, D3D_FEATURE_LEVEL_11_0, &IID_ID3D12Device,
        (void**)&value->device)) ? -6 : 0;
#endif
}

static int32_t CreateQueue(rsf_vk_xess_fg* value) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    int32_t code;
    RSF_BIND_COM(value->queue, rsf_core_xess_dx12_queue(value->device, &bound_));
    return code;
#else
    D3D12_COMMAND_QUEUE_DESC queue;
    memset(&queue, 0, sizeof(queue));
    queue.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    return FAILED(value->device->lpVtbl->CreateCommandQueue(value->device, &queue, &IID_ID3D12CommandQueue,
        (void**)&value->queue)) ? -7 : 0;
#endif
}

static int32_t CreateXell(rsf_vk_xess_fg* value) {
    const int code = (int)value->createXell(value->device, &value->xell);
    return code == 0 && !value->xell ? -8 : code;
}

static void LogSdkMessage(const char* message, xefg_swapchain_logging_level_t level, void* user) {
    (void)user;
    fprintf(stderr, "[xess-fg sdk %d] %s\n", (int)level, message);
    fflush(stderr);
}

static int32_t CreateFg(rsf_vk_xess_fg* value, void* module) {
    const int code = (int)value->createFg(value->device, &value->fg);
    if (code != 0 || !value->fg) return code != 0 ? code : -9;
    if (getenv("VULKANSTORY_XESS_FG_LOG")) {
        const FgSetLogging setLogging = (FgSetLogging)(uintptr_t)GetProcAddress((HMODULE)module,
            "xefgSwapChainSetLoggingCallback");
        if (setLogging) setLogging(value->fg, XEFG_SWAPCHAIN_LOGGING_LEVEL_DEBUG, LogSdkMessage, NULL);
    }
    return 0;
}

static int32_t SetLatency(rsf_vk_xess_fg* value) {
    return value->setLatency(value->fg, value->xell);
}

static int32_t SetSleepMode(rsf_vk_xess_fg* value, const xell_sleep_params_t* mode) {
    return value->setSleepMode(value->xell, mode);
}

static void LogCreateError(const char* stage, int32_t error, int32_t cleanup) {
    if (cleanup == 0)
        fprintf(stderr, "[xess-fg] initialization stage %s failed (%d)\n", stage, error);
    else
        fprintf(stderr, "[xess-fg] initialization failed (%d), release failed (%d); native owner retained\n",
            error, cleanup);
}

static int32_t ReadCompletion(rsf_vk_xess_fg* value, uint64_t* completed) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    return rsf_core_xess_dx12_completed(value->completionFence, completed);
#else
    if (!value->completionFence) return (int32_t)E_UNEXPECTED;
    *completed = value->completionFence->lpVtbl->GetCompletedValue(value->completionFence);
    return *completed == UINT64_MAX ? (int32_t)DXGI_ERROR_DEVICE_REMOVED : 0;
#endif
}

static int32_t DestroyFg(rsf_vk_xess_fg* value) {
    if (!value->fg) return 0;
    if (!value->destroyFg) return -1;
    const int code = (int)value->destroyFg(value->fg);
    if (code >= 0) value->fg = NULL;
    return code;
}

static int32_t DestroyXell(rsf_vk_xess_fg* value) {
    if (!value->xell) return 0;
    if (!value->destroyXell) return -1;
    const int code = (int)value->destroyXell(value->xell);
    if (code >= 0) value->xell = NULL;
    return code;
}

static bool IsStarted(const rsf_vk_xess_fg* value) {
    return rsf_core_xess_bridge_started(value->session) != 0;
}

RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_wait_idle(rsf_vk_xess_fg* value) {
    return value ? rsf_core_xess_bridge_wait_idle(value->session) : -1;
}

RSF_BRIDGE_EXPORT const char* rsf_vk_xess_fg_last_create_stage(void) {
    return rsf_core_xess_bridge_create_stage();
}

/* The host stops admission, then drains its Vulkan users before retiring the
   SDK consumers. Shared input resources stay alive until preparation succeeds. */
RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_prepare_destroy(rsf_vk_xess_fg* value) {
    return value ? rsf_core_xess_bridge_prepare_destroy(value->session) : 0;
}

RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_destroy_checked(rsf_vk_xess_fg* value) {
    return value ? rsf_core_xess_bridge_destroy(value->session) : 0;
}

RSF_BRIDGE_EXPORT void rsf_vk_xess_fg_destroy(rsf_vk_xess_fg* value) {
    rsf_vk_xess_fg_destroy_checked(value);
}

static int CreateContext(uint64_t adapterLuid, const wchar_t* directory, rsf_vk_xess_fg** output);

RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_create(uint64_t adapterLuid, rsf_vk_xess_fg** output) {
    return CreateContext(adapterLuid, NULL, output);
}

RSF_XESS_BRIDGE_API uint64_t RsfXessBridgeAdapterLuid(const rsf_xess_bridge* value) {
    DXGI_ADAPTER_DESC1 desc;
    memset(&desc, 0, sizeof(desc));
    if (!value || !value->adapter || FAILED(value->adapter->lpVtbl->GetDesc1(value->adapter, &desc))) return 0;
    uint64_t luid = 0;
    _Static_assert(sizeof(luid) == sizeof(desc.AdapterLuid), "adapter LUID size");
    memcpy(&luid, &desc.AdapterLuid, sizeof(luid));
    return luid;
}

/* Queries the selected Vulkan device LUID and creates its matching native
   presentation context. The Vulkan physical device is borrowed and must expose
   a valid Windows LUID. Returns the native creation result, or a negative
   argument or Vulkan-query availability code. */
RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_create_from_vulkan(VkPhysicalDevice physical, rsf_vk_xess_fg** output) {
    rsf_core_xess_bridge_note_vulkan_lookup();
    if (!output) return -1;
    *output = NULL;
    if (!physical) return -1;
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    uint64_t luid = 0;
    const int32_t status = rsf_core_xess_dx12_vulkan_luid(physical, &luid);
    if (status) return status;
#else
    HMODULE loader = GetModuleHandleW(L"vulkan-1.dll");
    const PFN_vkGetPhysicalDeviceProperties2 properties = loader ?
        (PFN_vkGetPhysicalDeviceProperties2)(uintptr_t)GetProcAddress(loader, "vkGetPhysicalDeviceProperties2") : NULL;
    if (!properties) return -10;
    VkPhysicalDeviceIDProperties id;
    memset(&id, 0, sizeof(id));
    id.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES;
    VkPhysicalDeviceProperties2 props;
    memset(&props, 0, sizeof(props));
    props.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2;
    props.pNext = &id;
    properties(physical, &props);
    if (!id.deviceLUIDValid) return -11;
    uint64_t luid = 0;
    memcpy(&luid, id.deviceLUID, VK_LUID_SIZE);
#endif
    return rsf_vk_xess_fg_create(luid, output);
}

/* The borrowed HWND must be free of other swapchain owners. Per-frame VSync
   is applied by the native presentation path. */
static int32_t InitSwapchain(rsf_vk_xess_fg* value, void* window, uint32_t width, uint32_t height,
    const xefg_swapchain_d3d12_init_params_t* params) {
    DXGI_SWAP_CHAIN_DESC1 desc;
    memset(&desc, 0, sizeof(desc));
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    value->allowTearing = rsf_core_xess_dx12_tearing(value->factory) != 0;
    rsf_core_xess_dx12_swapchain_desc(&desc, width, height, value->allowTearing);
#else
    IDXGIFactory5* factory5 = NULL;
    if (SUCCEEDED(value->factory->lpVtbl->QueryInterface(value->factory, &IID_IDXGIFactory5, (void**)&factory5))) {
        BOOL supported = FALSE;
        value->allowTearing = SUCCEEDED(factory5->lpVtbl->CheckFeatureSupport(factory5,
            DXGI_FEATURE_PRESENT_ALLOW_TEARING, &supported, sizeof(supported))) && supported;
        RSF_RELEASE(factory5);
    }
    desc.Width = width;
    desc.Height = height;
    desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    desc.SampleDesc.Count = 1;
    desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.BufferCount = 3;
    desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    desc.Flags = value->allowTearing ? DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING : 0;
#endif
    return (int32_t)value->initSwapchain(value->fg, (HWND)window, &desc, NULL, value->queue,
        (IDXGIFactory2*)value->factory, params);
}

static int32_t GetProperties(rsf_vk_xess_fg* value, xefg_swapchain_properties_t* properties) {
    return value->getProperties(value->fg, properties);
}

static int32_t GetSwapchain(rsf_vk_xess_fg* value) {
    const int code = (int)value->getSwapchain(value->fg, &IID_IDXGISwapChain4, (void**)&value->swapchain);
    return code >= 0 && !value->swapchain ? -2 : code;
}

static int32_t EnableUi(rsf_vk_xess_fg* value) {
    return value->setUi(value->fg, XEFG_SWAPCHAIN_UI_COMPOSITION_STATE_ENABLED);
}

static int32_t CreateCompletionFence(rsf_vk_xess_fg* value) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    int32_t code;
    RSF_BIND_COM(value->completionFence, rsf_core_xess_dx12_fence(value->device, 0, &bound_));
    return code;
#else
    return value->device->lpVtbl->CreateFence(value->device, 0, D3D12_FENCE_FLAG_NONE, &IID_ID3D12Fence,
        (void**)&value->completionFence);
#endif
}

static void* OpenCompletionEvent(void) {
    return CreateEventW(NULL, FALSE, FALSE, NULL);
}

static int32_t CreateRecorders(rsf_vk_xess_fg* value) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    void* allocators[RSF_XESS_BRIDGE_ALLOCATOR_COUNT] = {0};
    int32_t code;
    RSF_BIND_COM(value->commandList, rsf_core_xess_dx12_recorders(value->device, &allocators, &bound_));
    for (size_t i = 0; i < RSF_XESS_BRIDGE_ALLOCATOR_COUNT; ++i)
        value->allocators[i] = (ID3D12CommandAllocator*)allocators[i];
    return code;
#else
    HRESULT result;
    for (size_t i = 0; i < RSF_XESS_BRIDGE_ALLOCATOR_COUNT; ++i) {
        result = value->device->lpVtbl->CreateCommandAllocator(value->device, D3D12_COMMAND_LIST_TYPE_DIRECT,
            &IID_ID3D12CommandAllocator, (void**)&value->allocators[i]);
        if (FAILED(result)) return (int)result;
    }
    result = value->device->lpVtbl->CreateCommandList(value->device, 0, D3D12_COMMAND_LIST_TYPE_DIRECT,
        value->allocators[0], NULL, &IID_ID3D12GraphicsCommandList, (void**)&value->commandList);
    if (FAILED(result)) return (int)result;
    return (int)value->commandList->lpVtbl->Close(value->commandList);
#endif
}

/* The session admits the exact SDK binary before resolving its private setter. */
static void BindDiagnostics(rsf_vk_xess_fg* value, void* address) {
    value->setDiagnostics = (rsf_xess_set_diagnostics)(uintptr_t)address;
}

/* The SDK user pointer is the Rust capture owner. */
static void CaptureInput(void* user, uint64_t token, uint32_t kind,
    const xefg_swapchain_d3d12_resource_data_t* data, ID3D12GraphicsCommandList* commands) {
    rsf_core_xess_capture_input((rsf_xess_capture_owner*)user, token, kind, data, commands);
}

static void CaptureOutput(void* user, uint64_t token, uint32_t kind, ID3D12Resource* resource,
    ID3D12GraphicsCommandList* commands, uint32_t ordinal) {
    rsf_core_xess_capture_output((rsf_xess_capture_owner*)user, token, kind, resource, commands, ordinal);
}

static void CaptureBeforeOutputs(void* user, uint64_t token, ID3D12GraphicsCommandList* commands, uint32_t count) {
    (void)commands;
    if (user) rsf_core_xess_capture_before_outputs((rsf_xess_capture_owner*)user, token, count);
}

static int32_t RegisterDiagnostics(rsf_vk_xess_fg* value) {
    rsf_xess_diagnostics_v2 diagnostics;
    memset(&diagnostics, 0, sizeof(diagnostics));
    diagnostics.version = RSF_XESS_DIAGNOSTICS_VERSION;
    diagnostics.user = value->capture;
    diagnostics.input = CaptureInput;
    diagnostics.before_outputs = CaptureBeforeOutputs;
    diagnostics.output = CaptureOutput;
    const int result = value->setDiagnostics(value->fg, &diagnostics);
    if (result >= 0) value->diagnosticContext = value->fg;
    return result;
}

static int32_t UnregisterDiagnostics(rsf_vk_xess_fg* value) {
    if (!value->diagnosticContext || !value->setDiagnostics) return 0;
    const int result = value->setDiagnostics(value->diagnosticContext, NULL);
    if (result >= 0) value->diagnosticContext = NULL;
    return result;
}

static uint32_t CaptureDigest(void* module, unsigned char* sha256) {
    wchar_t path[32768];
    const DWORD size = GetModuleFileNameW((HMODULE)module, path, 32768);
    if (!size || size >= 32768) return 0;
    return rsf_common_sha256_file(path, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, sha256);
}

static int32_t SetGenerated(rsf_vk_xess_fg* value, uint32_t count) {
    return (int)value->setInterpolations(value->fg, count);
}

RSF_XESS_BRIDGE_API int32_t RsfXessBridgeCaptureSource(rsf_xess_bridge* value, const rsf_frame_record* record) {
    return value ? rsf_core_xess_capture_source(value->capture, record,
                       rsf_core_xess_bridge_active_generated(value->session))
                 : RSF_XESS_BRIDGE_INVALID_ARGUMENT;
}

RSF_XESS_BRIDGE_API uint64_t RsfXessBridgeCapturedCallbacks(const rsf_xess_bridge* value) {
    return value ? rsf_core_xess_capture_callbacks(value->capture) : 0;
}

RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_start(rsf_vk_xess_fg* value, HWND window, uint32_t width, uint32_t height,
    uint32_t vsync) {
    (void)vsync;
    return value ? rsf_core_xess_bridge_start(value->session, window, width, height, 0, 0,
        XEFG_SWAPCHAIN_RESULT_ERROR_UNSUPPORTED) : -1;
}

RSF_XESS_BRIDGE_API int32_t RsfXessBridgeStart(rsf_xess_bridge* value, const rsf_xess_bridge_start* setup) {
    return rsf_core_xess_bridge_start_integrated(value ? value->session : NULL, setup,
        XEFG_SWAPCHAIN_RESULT_ERROR_UNSUPPORTED);
}

RSF_XESS_BRIDGE_API uint32_t RsfXessBridgeMaxGenerated(const rsf_xess_bridge* value) {
    return value ? rsf_core_xess_bridge_max_generated(value->session) : 0;
}

RSF_XESS_BRIDGE_API int32_t RsfXessBridgeSetGenerated(rsf_xess_bridge* value, uint32_t count) {
    return value ? rsf_core_xess_bridge_set_generated(value->session, count, XEFG_SWAPCHAIN_RESULT_ERROR_UNSUPPORTED)
                 : RSF_XESS_BRIDGE_INVALID_ARGUMENT;
}

/* Forwards frame-generation enablement for an already started native presenter.
   Returns the SDK result, or -1 without a started FG context. */
RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_set_enabled(rsf_vk_xess_fg* value, uint32_t enabled) {
    return value && value->fg && IsStarted(value) ? (int)value->setEnabled(value->fg, enabled ? 1 : 0) : -1;
}

/* Clamps the interpolation count to the started runtime's supported range and
   avoids redundant SDK updates. effective and maximum receive the clamped count
   and the native limit. Returns the SDK result, zero for an unchanged count, or
   -1 for missing prerequisites. */
RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_set_generated_frames(rsf_vk_xess_fg* value, uint32_t requested,
    uint32_t* effective, uint32_t* maximum) {
    return value && value->fg ?
        rsf_core_xess_bridge_set_generated_legacy(value->session, requested, effective, maximum) : -1;
}

/* VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT is a handle to a D3D12
   committed resource created by ID3D12Device::CreateSharedHandle. Vulkan imports
   this handle; it is not a handle exported from an arbitrary Vulkan allocation.
   Creates a COMMON-state committed texture with the Rust format plan. The caller
   closes sharedHandle after import and releases resource after both APIs finish.
   Outputs are cleared before creation. Returns zero on success. */
RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_create_shared_image(rsf_vk_xess_fg* value, uint32_t width, uint32_t height,
    uint32_t vkFormat, HANDLE* sharedHandle, ID3D12Resource** resource) {
    if (!value || !value->device || !width || !height || !sharedHandle || !resource) return -1;
    *sharedHandle = NULL;
    *resource = NULL;
    _Static_assert(VK_FORMAT_R8G8B8A8_UNORM == 37 && VK_FORMAT_B8G8R8A8_UNORM == 44 &&
        VK_FORMAT_R16G16_SFLOAT == 83 && VK_FORMAT_R16G16B16A16_SFLOAT == 97 &&
        VK_FORMAT_R32_SFLOAT == 100 && VK_FORMAT_D32_SFLOAT == 126, "Vulkan formats");
    _Static_assert(DXGI_FORMAT_R8G8B8A8_UNORM == 28 && DXGI_FORMAT_R16G16B16A16_FLOAT == 10 &&
        D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL == 2, "DXGI formats");
    rsf_xess_bridge_shared_image_desc plan;
    memset(&plan, 0, sizeof(plan));
    const int32_t planned = rsf_core_xess_bridge_shared_image_plan(width, height, vkFormat, &plan);
    if (planned != 0) return planned;
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    void* plannedResource = NULL;
    const int32_t result = rsf_core_xess_d3d12_shared_image(value->device, &plan, sharedHandle, &plannedResource);
    *resource = (ID3D12Resource*)plannedResource;
    return result;
#else
    D3D12_HEAP_PROPERTIES heap;
    memset(&heap, 0, sizeof(heap));
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc;
    memset(&desc, 0, sizeof(desc));
    desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = plan.width;
    desc.Height = plan.height;
    desc.DepthOrArraySize = 1;
    desc.MipLevels = 1;
    desc.Format = (DXGI_FORMAT)plan.format;
    desc.SampleDesc.Count = 1;
    desc.Layout = D3D12_TEXTURE_LAYOUT_UNKNOWN;
    desc.Flags = (D3D12_RESOURCE_FLAGS)plan.flags;
    HRESULT result = value->device->lpVtbl->CreateCommittedResource(value->device, &heap, D3D12_HEAP_FLAG_SHARED,
        &desc, D3D12_RESOURCE_STATE_COMMON, NULL, &IID_ID3D12Resource, (void**)resource);
    if (FAILED(result)) return (int)result;
    result = value->device->lpVtbl->CreateSharedHandle(value->device, (ID3D12DeviceChild*)*resource, NULL,
        GENERIC_ALL, NULL, sharedHandle);
    if (FAILED(result)) {
        RSF_RELEASE(*resource);
        return (int)result;
    }
    return 0;
#endif
}

/* Releases a caller-owned shared-image COM reference; null is accepted. No
   waiting occurs; the host must first complete Vulkan import users and DX12 consumers. */
RSF_BRIDGE_EXPORT void rsf_vk_xess_fg_release_image(ID3D12Resource* resource) {
    RSF_RELEASE(resource);
}

/* Creates or reuses the native-owned DX12 shared fence and exports a
   caller-owned NT handle. The importer closes the handle after permanent Vulkan
   timeline-semaphore import; the bridge keeps the fence for GPU queue waits and
   signals. Returns zero on success, or a local or HRESULT error. */
RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_create_shared_fence(rsf_vk_xess_fg* value, HANDLE* sharedHandle) {
    if (!value || !value->device || !sharedHandle) return -1;
    *sharedHandle = NULL;
    if (!value->sharedFence) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
        int32_t code;
        RSF_BIND_COM(value->sharedFence, rsf_core_xess_dx12_fence(value->device, 1, &bound_));
        const HRESULT result = code;
#else
        const HRESULT result = value->device->lpVtbl->CreateFence(value->device, 0, D3D12_FENCE_FLAG_SHARED,
            &IID_ID3D12Fence, (void**)&value->sharedFence);
#endif
        if (FAILED(result)) return (int)result;
    }
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    return rsf_core_xess_dx12_share_fence(value->device, value->sharedFence, sharedHandle);
#else
    const HRESULT result = value->device->lpVtbl->CreateSharedHandle(value->device,
        (ID3D12DeviceChild*)value->sharedFence, NULL, GENERIC_ALL, NULL, sharedHandle);
    return FAILED(result) ? (int)result : 0;
#endif
}

static int32_t WaitShared(rsf_vk_xess_fg* value, uint64_t target) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    return rsf_core_xess_dx12_wait(value->queue, value->sharedFence, target);
#else
    return (int32_t)value->queue->lpVtbl->Wait(value->queue, value->sharedFence, target);
#endif
}

static int32_t SignalDone(rsf_vk_xess_fg* value, uint64_t target) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    return rsf_core_xess_dx12_signal(value->queue, value->sharedFence, target);
#else
    return (int32_t)value->queue->lpVtbl->Signal(value->queue, value->sharedFence, target);
#endif
}

/* Queues a DX12 GPU wait for the borrowed shared-fence value. Returns after
   enqueuing; it does not wait on the CPU. */
RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_wait_shared_fence(rsf_vk_xess_fg* value, uint64_t fenceValue) {
    if (!value || !value->queue || !value->sharedFence || !fenceValue) return -1;
    const int32_t code = WaitShared(value, fenceValue);
    return code < 0 ? code : 0;
}

/* Queues a DX12 shared-fence signal behind prior queue work. The caller reserves
   ordered values; a successful enqueue is not GPU completion. */
RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_signal_shared_fence(rsf_vk_xess_fg* value, uint64_t fenceValue) {
    if (!value || !value->queue || !value->sharedFence || !fenceValue) return -1;
    const int32_t code = SignalDone(value, fenceValue);
    return code < 0 ? code : 0;
}

#if !defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
/* Records an all-subresource state transition for a borrowed DX12 resource. */
static void Transition(ID3D12GraphicsCommandList* list, ID3D12Resource* resource,
    D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
    D3D12_RESOURCE_BARRIER barrier;
    memset(&barrier, 0, sizeof(barrier));
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition.pResource = resource;
    barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barrier.Transition.StateBefore = before;
    barrier.Transition.StateAfter = after;
    list->lpVtbl->ResourceBarrier(list, 1, &barrier);
}

static D3D12_RESOURCE_DESC ResourceDesc(ID3D12Resource* resource) {
    D3D12_RESOURCE_DESC desc;
    resource->lpVtbl->GetDesc(resource, &desc);
    return desc;
}

/* Lazily creates opt-in timestamp queries, readback storage and post-present
   command lists. Objects stay owned by the context and are released during
   checked destruction; a false result disables that timing sample. */
static bool EnsureGpuTiming(rsf_vk_xess_fg* value) {
    if (value->postList) return true;
    enum { queryCount = RSF_XESS_BRIDGE_ALLOCATOR_COUNT * 3 };
    D3D12_QUERY_HEAP_DESC heap;
    memset(&heap, 0, sizeof(heap));
    heap.Type = D3D12_QUERY_HEAP_TYPE_TIMESTAMP;
    heap.Count = queryCount;
    if (FAILED(value->device->lpVtbl->CreateQueryHeap(value->device, &heap, &IID_ID3D12QueryHeap,
            (void**)&value->queryHeap))) return false;
    D3D12_HEAP_PROPERTIES readbackHeap;
    memset(&readbackHeap, 0, sizeof(readbackHeap));
    readbackHeap.Type = D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC buffer;
    memset(&buffer, 0, sizeof(buffer));
    buffer.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    buffer.Width = queryCount * sizeof(uint64_t);
    buffer.Height = 1;
    buffer.DepthOrArraySize = 1;
    buffer.MipLevels = 1;
    buffer.SampleDesc.Count = 1;
    buffer.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    if (FAILED(value->device->lpVtbl->CreateCommittedResource(value->device, &readbackHeap, D3D12_HEAP_FLAG_NONE,
            &buffer, D3D12_RESOURCE_STATE_COPY_DEST, NULL, &IID_ID3D12Resource, (void**)&value->queryReadback)))
        return false;
    for (size_t i = 0; i < RSF_XESS_BRIDGE_ALLOCATOR_COUNT; ++i)
        if (FAILED(value->device->lpVtbl->CreateCommandAllocator(value->device, D3D12_COMMAND_LIST_TYPE_DIRECT,
                &IID_ID3D12CommandAllocator, (void**)&value->postAllocators[i]))) return false;
    if (FAILED(value->device->lpVtbl->CreateCommandList(value->device, 0, D3D12_COMMAND_LIST_TYPE_DIRECT,
            value->postAllocators[0], NULL, &IID_ID3D12GraphicsCommandList, (void**)&value->postList)))
        return false;
    value->postList->lpVtbl->Close(value->postList);
    return SUCCEEDED(value->queue->lpVtbl->GetTimestampFrequency(value->queue, &value->gpuFrequency)) &&
        value->gpuFrequency;
}

/* The slot's previous present has completed on the DX12 queue: its timestamps
   are final and go on the CPU clock through a fresh calibration. The caller
   first waits for that completion before reusing query and allocator storage. */
static void HarvestGpuTiming(rsf_vk_xess_fg* value, uint32_t slot) {
    if (!value->slotTimed[slot]) return;
    value->slotTimed[slot] = false;
    uint64_t* mapped = NULL;
    const D3D12_RANGE range = {slot * 3 * sizeof(uint64_t), (slot * 3 + 3) * sizeof(uint64_t)};
    if (FAILED(value->queryReadback->lpVtbl->Map(value->queryReadback, 0, &range, (void**)&mapped))) return;
    const uint64_t gpuStart = mapped[slot * 3], gpuCopiesEnd = mapped[slot * 3 + 1], gpuEnd = mapped[slot * 3 + 2];
    const D3D12_RANGE none = {0, 0};
    value->queryReadback->lpVtbl->Unmap(value->queryReadback, 0, &none);
    uint64_t gpuCalibration = 0, cpuCalibration = 0;
    if (gpuEnd <= gpuStart ||
        FAILED(value->queue->lpVtbl->GetClockCalibration(value->queue, &gpuCalibration, &cpuCalibration))) return;
    const rsf_xess_timing_gpu_sample sample = {gpuStart, gpuCopiesEnd, gpuEnd, value->gpuFrequency,
        gpuCalibration, cpuCalibration, value->presentEnter[slot], value->presentExit[slot]};
    rsf_core_xess_timing_gpu(value->timing, &sample, TimingFrequency());
}

static void DescribePresentResource(void* resource, rsf_xess_bridge_resource_desc* output) {
    const D3D12_RESOURCE_DESC desc = ResourceDesc((ID3D12Resource*)resource);
    output->width = desc.Width;
    output->height = desc.Height;
    output->format = (uint32_t)desc.Format;
}

/* Standalone capture leaves; the integrated bridge uses the Runtime's typed calls. */
static void CaptureDescribe(void* resource, rsf_xess_capture_desc* output) {
    const D3D12_RESOURCE_DESC desc = ResourceDesc((ID3D12Resource*)resource);
    output->width = desc.Width;
    output->height = desc.Height;
    output->dimension = (uint32_t)desc.Dimension;
    output->format = (uint32_t)desc.Format;
    output->depth_or_array_size = desc.DepthOrArraySize;
    output->mip_levels = desc.MipLevels;
    output->sample_count = desc.SampleDesc.Count;
}

/* Returns a device reference for the readback allocation, or -1 without one. */
static int32_t CaptureFootprint(void* resource, void* footprint, uint64_t* bytes, void** device) {
    ID3D12Resource* source = (ID3D12Resource*)resource;
    ID3D12Device* owner = NULL;
    if (FAILED(source->lpVtbl->GetDevice(source, &IID_ID3D12Device, (void**)&owner))) return -1;
    const D3D12_RESOURCE_DESC desc = ResourceDesc(source);
    UINT rows = 0;
    UINT64 rowBytes = 0;
    owner->lpVtbl->GetCopyableFootprints(owner, &desc, 0, 1, 0, (D3D12_PLACED_SUBRESOURCE_FOOTPRINT*)footprint,
        &rows, &rowBytes, bytes);
    *device = owner;
    return 0;
}

static void* CaptureReadback(void* device, uint64_t bytes) {
    D3D12_HEAP_PROPERTIES heap;
    memset(&heap, 0, sizeof(heap));
    heap.Type = D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC buffer;
    memset(&buffer, 0, sizeof(buffer));
    buffer.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    buffer.Width = bytes;
    buffer.Height = buffer.DepthOrArraySize = buffer.MipLevels = 1;
    buffer.SampleDesc.Count = 1;
    buffer.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ID3D12Device* owner = (ID3D12Device*)device;
    ID3D12Resource* readback = NULL;
    const HRESULT created = owner->lpVtbl->CreateCommittedResource(owner, &heap, D3D12_HEAP_FLAG_NONE, &buffer,
        D3D12_RESOURCE_STATE_COPY_DEST, NULL, &IID_ID3D12Resource, (void**)&readback);
    if (SUCCEEDED(created)) return readback;
    RSF_RELEASE(readback);
    return NULL;
}

static void CaptureRelease(void* object) {
    RSF_RELEASE(object);
}

/* Records into the borrowed SDK command list and restores the callback state. */
static void CaptureCopy(void* commands, void* resource, void* readback, const void* footprint, uint32_t state) {
    ID3D12GraphicsCommandList* list = (ID3D12GraphicsCommandList*)commands;
    ID3D12Resource* source = (ID3D12Resource*)resource;
    const D3D12_RESOURCE_STATES incoming = (D3D12_RESOURCE_STATES)state;
    if (incoming != D3D12_RESOURCE_STATE_COPY_SOURCE)
        Transition(list, source, incoming, D3D12_RESOURCE_STATE_COPY_SOURCE);
    D3D12_TEXTURE_COPY_LOCATION from;
    memset(&from, 0, sizeof(from));
    from.pResource = source;
    from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    D3D12_TEXTURE_COPY_LOCATION to;
    memset(&to, 0, sizeof(to));
    to.pResource = (ID3D12Resource*)readback;
    to.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    to.PlacedFootprint = *(const D3D12_PLACED_SUBRESOURCE_FOOTPRINT*)footprint;
    list->lpVtbl->CopyTextureRegion(list, &to, 0, 0, 0, &from, NULL);
    if (incoming != D3D12_RESOURCE_STATE_COPY_SOURCE)
        Transition(list, source, D3D12_RESOURCE_STATE_COPY_SOURCE, incoming);
}

/* Called only after the bridge drained its DX12 queue and completed its fence. */
static int32_t CaptureMap(void* readback, uint64_t bytes, const uint8_t** output) {
    ID3D12Resource* resource = (ID3D12Resource*)readback;
    void* pixels = NULL;
    const D3D12_RANGE range = {0, (SIZE_T)bytes};
    if (FAILED(resource->lpVtbl->Map(resource, 0, &range, &pixels))) return -1;
    *output = (const uint8_t*)pixels;
    return 0;
}

static void CaptureUnmap(void* readback) {
    ID3D12Resource* resource = (ID3D12Resource*)readback;
    const D3D12_RANGE written = {0, 0};
    resource->lpVtbl->Unmap(resource, 0, &written);
}
#endif

_Static_assert(DXGI_FORMAT_B8G8R8A8_UNORM == 87 && DXGI_FORMAT_R16G16_FLOAT == 34 &&
    DXGI_FORMAT_D32_FLOAT == 40 && DXGI_FORMAT_R32_FLOAT == 41, "DXGI guide formats");

static int32_t ResetPresentCommands(rsf_vk_xess_fg* value, uint32_t slot) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    return rsf_core_xess_dx12_reset(value->allocators[slot], value->commandList);
#else
    ID3D12CommandAllocator* allocator = value->allocators[slot];
    const HRESULT result = allocator->lpVtbl->Reset(allocator);
    if (FAILED(result)) return (int32_t)result;
    return (int32_t)value->commandList->lpVtbl->Reset(value->commandList, allocator, NULL);
#endif
}

_Static_assert(D3D12_RESOURCE_STATE_COMMON == 0 && D3D12_RESOURCE_STATE_PRESENT == 0 &&
    D3D12_RESOURCE_STATE_COPY_DEST == 0x400 && D3D12_RESOURCE_STATE_COPY_SOURCE == 0x800, "D3D12 states");

static void TransitionPresentResource(rsf_vk_xess_fg* value, void* resource, uint32_t before, uint32_t after) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    rsf_core_xess_d3d12_present_transition(value->commandList, resource, before, after);
#else
    Transition(value->commandList, (ID3D12Resource*)resource, (D3D12_RESOURCE_STATES)before,
        (D3D12_RESOURCE_STATES)after);
#endif
}

static int32_t AcquireBackbuffer(rsf_vk_xess_fg* value, void** output) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    return rsf_core_xess_dx12_backbuffer(value->swapchain, output);
#else
    ID3D12Resource* backbuffer = NULL;
    const UINT backbufferIndex = value->swapchain->lpVtbl->GetCurrentBackBufferIndex(value->swapchain);
    const HRESULT result = value->swapchain->lpVtbl->GetBuffer(value->swapchain, backbufferIndex,
        &IID_ID3D12Resource, (void**)&backbuffer);
    if (SUCCEEDED(result)) *output = backbuffer;
    return (int32_t)result;
#endif
}

static void CopyPresentColor(rsf_vk_xess_fg* value, void* backbuffer, void* color) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    rsf_core_xess_d3d12_present_copy(value->commandList, backbuffer, color);
#else
    value->commandList->lpVtbl->CopyResource(value->commandList, (ID3D12Resource*)backbuffer,
        (ID3D12Resource*)color);
#endif
}

static int32_t TagPresentResource(rsf_vk_xess_fg* value, uint32_t frameId,
    const xefg_swapchain_d3d12_resource_data_t* data) {
    return value->tagResource(value->fg, (ID3D12CommandList*)value->commandList, frameId, data);
}

static void ReleaseBackbuffer(void* backbuffer) {
    RSF_RELEASE(backbuffer);
}

static int32_t CloseCommands(rsf_vk_xess_fg* value) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    return rsf_core_xess_dx12_close(value->commandList);
#else
    return value->commandList->lpVtbl->Close(value->commandList);
#endif
}

static int32_t TagConstants(rsf_vk_xess_fg* value, uint32_t frameId,
    const xefg_swapchain_frame_constant_data_t* data) {
    return value->tagConstants(value->fg, frameId, data);
}

static void ExecutePresentCommands(rsf_vk_xess_fg* value) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    rsf_core_xess_dx12_execute(value->queue, value->commandList);
#else
    ID3D12CommandList* lists[] = {(ID3D12CommandList*)value->commandList};
    value->queue->lpVtbl->ExecuteCommandLists(value->queue, 1, lists);
#endif
}

static int32_t SetPresentId(rsf_vk_xess_fg* value, uint32_t frameId) {
    return value->setPresentId(value->fg, frameId);
}

static int32_t PresentSwapchain(rsf_vk_xess_fg* value, uint32_t vsync) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    return rsf_core_xess_dx12_present(value->swapchain, vsync, value->allowTearing);
#else
    const UINT flags = !vsync && value->allowTearing ? DXGI_PRESENT_ALLOW_TEARING : 0;
    return (int32_t)value->swapchain->lpVtbl->Present(value->swapchain, vsync ? 1 : 0, flags);
#endif
}

static int32_t SignalCompletion(rsf_vk_xess_fg* value, uint64_t target) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    return rsf_core_xess_dx12_signal(value->queue, value->completionFence, target);
#else
    return value->queue->lpVtbl->Signal(value->queue, value->completionFence, target);
#endif
}

static int32_t ReadPresentStatus(rsf_vk_xess_fg* value, xefg_swapchain_present_status_t* status) {
    return value->presentStatus(value->fg, status);
}

static int32_t ArmCompletion(rsf_vk_xess_fg* value, uint64_t target, void* event) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    return rsf_core_xess_dx12_arm(value->completionFence, target, event);
#else
    return value->completionFence->lpVtbl->SetEventOnCompletion(value->completionFence, target, event);
#endif
}

static int32_t WaitEvent(void* event, uint32_t timeout) {
    return WaitForSingleObject(event, timeout) == WAIT_OBJECT_0 ? 0 : -3;
}

static int32_t DisableFg(rsf_vk_xess_fg* value) {
    return value->fg && value->setEnabled ? (int)value->setEnabled(value->fg, 0) : -1;
}

static void ReleaseSwapchain(rsf_vk_xess_fg* value) {
    RSF_RELEASE(value->swapchain);
}

static uint32_t HasSharedQueue(const rsf_vk_xess_fg* value) {
    return value->queue && value->sharedFence;
}

static uint32_t BeginGpuTiming(rsf_vk_xess_fg* value, uint32_t slot) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    return rsf_core_xess_dx12_timestamps_begin(&value->timestamps, value->device, value->queue, slot, value->timing,
        TimingFrequency());
#else
    if (!EnsureGpuTiming(value)) return 0;
    HarvestGpuTiming(value, slot);
    return 1;
#endif
}

static void TimestampCopies(rsf_vk_xess_fg* value, uint32_t slot, uint32_t index) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    rsf_core_xess_dx12_timestamps_copy(value->timestamps, value->commandList, slot, index);
#else
    value->commandList->lpVtbl->EndQuery(value->commandList, value->queryHeap, D3D12_QUERY_TYPE_TIMESTAMP,
        slot * 3 + index);
#endif
}

static void MarkPresentEnter(rsf_vk_xess_fg* value, uint32_t slot) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    rsf_core_xess_dx12_timestamps_enter(value->timestamps, slot, TimingNow());
#else
    value->presentEnter[slot] = TimingNow();
#endif
}

static void FinishGpuTiming(rsf_vk_xess_fg* value, uint32_t slot) {
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    rsf_core_xess_dx12_timestamps_finish(value->timestamps, value->queue, slot, TimingNow());
#else
    value->presentExit[slot] = TimingNow();
    ID3D12CommandAllocator* allocator = value->postAllocators[slot];
    ID3D12GraphicsCommandList* post = value->postList;
    if (SUCCEEDED(allocator->lpVtbl->Reset(allocator)) &&
        SUCCEEDED(post->lpVtbl->Reset(post, allocator, NULL))) {
        post->lpVtbl->EndQuery(post, value->queryHeap, D3D12_QUERY_TYPE_TIMESTAMP, slot * 3 + 2);
        post->lpVtbl->ResolveQueryData(post, value->queryHeap, D3D12_QUERY_TYPE_TIMESTAMP, slot * 3, 3,
            value->queryReadback, slot * 3 * sizeof(uint64_t));
        if (SUCCEEDED(post->lpVtbl->Close(post))) {
            ID3D12CommandList* lists[] = {(ID3D12CommandList*)post};
            value->queue->lpVtbl->ExecuteCommandLists(value->queue, 1, lists);
            value->slotTimed[slot] = true;
        }
    }
#endif
}

static void AttachCapture(rsf_vk_xess_fg* value, rsf_xess_capture_owner* capture) {
    value->capture = capture;
}

static const rsf_xess_bridge_platform Platform = {
    .allocate = Allocate,
    .release_native = ReleaseNative,
    .load_runtime = LoadRuntime,
    .unload_module = UnloadModule,
    .create_factory = CreateFactory,
    .find_adapter = FindAdapter,
    .create_device = CreateDevice,
    .create_queue = CreateQueue,
    .create_xell = CreateXell,
    .create_fg = CreateFg,
    .set_latency = SetLatency,
    .set_sleep_mode = SetSleepMode,
    .log_create_error = LogCreateError,
    .init_swapchain = InitSwapchain,
    .get_properties = GetProperties,
    .get_swapchain = GetSwapchain,
    .enable_ui = EnableUi,
    .create_completion_fence = CreateCompletionFence,
    .open_event = OpenCompletionEvent,
    .create_recorders = CreateRecorders,
    .bind_diagnostics = BindDiagnostics,
    .set_generated = SetGenerated,
    .read_completion = ReadCompletion,
    .arm_completion = ArmCompletion,
    .wait_event = WaitEvent,
    .disable_fg = DisableFg,
    .unregister_diagnostics = UnregisterDiagnostics,
    .release_swapchain = ReleaseSwapchain,
    .destroy_fg = DestroyFg,
    .destroy_xell = DestroyXell,
    .has_shared_queue = HasSharedQueue,
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    .describe_present_resource = rsf_core_xess_d3d12_present_describe,
#else
    .describe_present_resource = DescribePresentResource,
#endif
    .reset_commands = ResetPresentCommands,
    .transition_present_resource = TransitionPresentResource,
    .acquire_backbuffer = AcquireBackbuffer,
    .copy_present_color = CopyPresentColor,
    .tag_present_resource = TagPresentResource,
    .release_backbuffer = ReleaseBackbuffer,
    .close_commands = CloseCommands,
    .tag_constants = TagConstants,
    .wait_shared = WaitShared,
    .execute = ExecutePresentCommands,
    .set_present_id = SetPresentId,
    .present = PresentSwapchain,
    .signal_done = SignalDone,
    .signal_completion = SignalCompletion,
    .read_present_status = ReadPresentStatus,
    .timing_enabled = TimingEnabled,
    .timing_now = TimingNow,
    .timing_frequency = TimingFrequency,
    .timing_report = WriteTimingReport,
    .begin_gpu_timing = BeginGpuTiming,
    .timestamp_copies = TimestampCopies,
    .mark_present_enter = MarkPresentEnter,
    .finish_gpu_timing = FinishGpuTiming,
    .serialize_enabled = SerializeEnabled,
    .attach_capture = AttachCapture,
    .register_diagnostics = RegisterDiagnostics,
    .capture_digest = CaptureDigest,
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
    .capture_describe = rsf_core_xess_d3d12_capture_describe,
    .capture_footprint = rsf_core_xess_d3d12_capture_footprint,
    .capture_readback = rsf_core_xess_d3d12_capture_readback,
    .capture_release = rsf_core_xess_d3d12_capture_release,
    .capture_copy = rsf_core_xess_d3d12_capture_copy,
    .capture_map = rsf_core_xess_d3d12_capture_map,
    .capture_unmap = rsf_core_xess_d3d12_capture_unmap,
#else
    .capture_describe = CaptureDescribe,
    .capture_footprint = CaptureFootprint,
    .capture_readback = CaptureReadback,
    .capture_release = CaptureRelease,
    .capture_copy = CaptureCopy,
    .capture_map = CaptureMap,
    .capture_unmap = CaptureUnmap,
#endif
    .lookup_export = LookupExport,
    .bind_runtime = BindRuntime,
};

static int CreateContext(uint64_t adapterLuid, const wchar_t* directory, rsf_vk_xess_fg** output) {
    return rsf_core_xess_bridge_create(&Platform, adapterLuid, (const uint16_t*)directory, output);
}

RSF_XESS_BRIDGE_API int32_t RsfXessBridgeCreate(const rsf_xess_bridge_create* setup, rsf_xess_bridge** output) {
    return rsf_core_xess_bridge_create_integrated(&Platform, setup, output);
}

/* Presents a prepared frame and preserves the ordered shared-fence handoff on a
   recoverable failure: the session queues readiness, done and local completion
   in order while Vulkan still waits for the handoff. Host calls are serialized.
   The status outputs are required; the return value is the presentation and
   status-query result. */
RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_present(rsf_vk_xess_fg* value, const rsf_vk_xess_fg_frame* frame,
    uint32_t* framesPresented, int* frameGenResult, uint32_t* frameGenEnabled) {
    return rsf_core_xess_bridge_present(value ? value->session : NULL, frame, framesPresented, frameGenResult,
        frameGenEnabled);
}

RSF_XESS_BRIDGE_API int32_t RsfXessBridgePresent(rsf_xess_bridge* value, const rsf_xess_bridge_frame* frame,
    rsf_xess_bridge_present_status* status) {
    return rsf_core_xess_bridge_present_integrated(value ? value->session : NULL, frame, status);
}

/* Forwards XeLL sleep-mode and minimum-interval options. The host applies SDK
   options at an idle lifecycle boundary; this does not drain the GPU. Returns
   the SDK result, or -1 without a XeLL context. */
RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_set_latency_mode(rsf_vk_xess_fg* value, uint32_t minimumIntervalUs,
    uint32_t enabled) {
    if (!value || !value->xell) return -1;
    return rsf_core_xess_bridge_set_sleep_mode(value->session, minimumIntervalUs, enabled);
}

/* Invokes XeLL sleep for the supplied frame before host input polling. Optional
   timing measures the CPU call duration only. */
RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_sleep(rsf_vk_xess_fg* value, uint32_t frameId) {
    if (!value || !value->xell) return -1;
    const bool timed = TimingEnabled() != 0;
    const int64_t start = timed ? TimingNow() : 0;
    const int result = (int)value->sleep(value->xell, frameId);
    if (timed) rsf_core_xess_timing_cpu(value->timing, kSleep, start, TimingNow(), TimingFrequency());
    return result;
}

/* Emits the supplied XeLL marker for the matching frame identity. */
RSF_BRIDGE_EXPORT int rsf_vk_xess_fg_marker(rsf_vk_xess_fg* value, uint32_t frameId,
    xell_latency_marker_type_t marker) {
    return value && value->xell ? (int)value->marker(value->xell, frameId, marker) : -1;
}
