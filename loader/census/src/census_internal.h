// SPDX-License-Identifier: GPL-3.0-only
// Internal types shared by the census translation units. Nothing here crosses the DLL boundary.
#pragma once

#include <windows.h>

#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <type_traits>

#include "rescaleframe/vendor_census.h"

namespace rsf_census {

// ---- Intercepted functions --------------------------------------------------------------------
// X(id, api, export name or nullptr, modules). `modules` is a '|' separated list of lowercase
// module basenames whose exports are detoured when the module loads. An entry with no export name
// is bound by the DXGI and Vulkan code that learns the address at run time.

#define RSF_NGX_FUNCS(X, A, API, MODS)                                                          \
    X(Ngx##A##Init, API, "NVSDK_NGX_" #A "_Init", MODS)                                        \
    X(Ngx##A##InitExt, API, "NVSDK_NGX_" #A "_Init_Ext", MODS)                                 \
    X(Ngx##A##InitProjectId, API, "NVSDK_NGX_" #A "_Init_ProjectID", MODS)                     \
    X(Ngx##A##InitWithProjectId, API, "NVSDK_NGX_" #A "_Init_with_ProjectID", MODS)            \
    X(Ngx##A##Shutdown, API, "NVSDK_NGX_" #A "_Shutdown", MODS)                                \
    X(Ngx##A##Shutdown1, API, "NVSDK_NGX_" #A "_Shutdown1", MODS)                              \
    X(Ngx##A##GetParameters, API, "NVSDK_NGX_" #A "_GetParameters", MODS)                      \
    X(Ngx##A##AllocateParameters, API, "NVSDK_NGX_" #A "_AllocateParameters", MODS)            \
    X(Ngx##A##GetCapabilityParameters, API, "NVSDK_NGX_" #A "_GetCapabilityParameters", MODS)  \
    X(Ngx##A##DestroyParameters, API, "NVSDK_NGX_" #A "_DestroyParameters", MODS)              \
    X(Ngx##A##CreateFeature, API, "NVSDK_NGX_" #A "_CreateFeature", MODS)                      \
    X(Ngx##A##CreateFeature1, API, "NVSDK_NGX_" #A "_CreateFeature1", MODS)                    \
    X(Ngx##A##EvaluateFeature, API, "NVSDK_NGX_" #A "_EvaluateFeature", MODS)                  \
    X(Ngx##A##ReleaseFeature, API, "NVSDK_NGX_" #A "_ReleaseFeature", MODS)

#define RSF_NGX_MODULES "nvngx.dll|_nvngx.dll"

#define RSF_CENSUS_FUNCTIONS(X)                                                                 \
    X(CreateDXGIFactory, "dxgi", "CreateDXGIFactory", "dxgi.dll")                               \
    X(CreateDXGIFactory1, "dxgi", "CreateDXGIFactory1", "dxgi.dll")                             \
    X(CreateDXGIFactory2, "dxgi", "CreateDXGIFactory2", "dxgi.dll")                             \
    X(D3D12CreateDevice, "d3d12", "D3D12CreateDevice", "d3d12.dll")                             \
    X(DxgiFactoryCreateSwapChain, "dxgi", nullptr, "")                                          \
    X(DxgiFactoryCreateSwapChainForHwnd, "dxgi", nullptr, "")                                   \
    X(DxgiFactoryCreateSwapChainForCoreWindow, "dxgi", nullptr, "")                             \
    X(DxgiSwapChainPresent, "dxgi", nullptr, "")                                                \
    X(DxgiSwapChainPresent1, "dxgi", nullptr, "")                                               \
    X(DxgiSwapChainResizeBuffers, "dxgi", nullptr, "")                                          \
    X(DxgiSwapChainResizeBuffers1, "dxgi", nullptr, "")                                         \
    X(DxgiSwapChainSetColorSpace1, "dxgi", nullptr, "")                                         \
    X(DxgiSwapChainSetFullscreenState, "dxgi", nullptr, "")                                     \
    X(DxgiSwapChainGetFrameLatencyWaitableObject, "dxgi", nullptr, "")                          \
    X(D3d12DeviceCreateCommandQueue, "d3d12", nullptr, "")                                      \
    X(D3d12QueueExecuteCommandLists, "d3d12", nullptr, "")                                      \
    X(VkGetInstanceProcAddr, "vulkan", "vkGetInstanceProcAddr", "vulkan-1.dll")                 \
    X(VkGetDeviceProcAddr, "vulkan", "vkGetDeviceProcAddr", "vulkan-1.dll")                     \
    X(VkCreateInstance, "vulkan", "vkCreateInstance", "")                                       \
    X(VkCreateDevice, "vulkan", "vkCreateDevice", "")                                           \
    X(VkGetDeviceQueue, "vulkan", "vkGetDeviceQueue", "")                                       \
    X(VkGetDeviceQueue2, "vulkan", "vkGetDeviceQueue2", "")                                     \
    X(VkQueueSubmit, "vulkan", "vkQueueSubmit", "")                                             \
    X(VkQueueSubmit2, "vulkan", "vkQueueSubmit2", "")                                           \
    X(VkQueueSubmit2KHR, "vulkan", "vkQueueSubmit2KHR", "")                                     \
    X(VkQueuePresentKHR, "vulkan", "vkQueuePresentKHR", "")                                     \
    X(VkCreateSwapchainKHR, "vulkan", "vkCreateSwapchainKHR", "")                               \
    RSF_NGX_FUNCS(X, D3D12, "ngx_d3d12", RSF_NGX_MODULES)                                       \
    RSF_NGX_FUNCS(X, D3D11, "ngx_d3d11", RSF_NGX_MODULES)                                       \
    RSF_NGX_FUNCS(X, VULKAN, "ngx_vulkan", RSF_NGX_MODULES)                                     \
    X(Fsr2ContextCreate, "fsr2", "ffxFsr2ContextCreate", "ffx_fsr2_api_x64.dll")                \
    X(Fsr2ContextDispatch, "fsr2", "ffxFsr2ContextDispatch", "ffx_fsr2_api_x64.dll")            \
    X(Fsr2ContextGenerateReactiveMask, "fsr2", "ffxFsr2ContextGenerateReactiveMask",            \
      "ffx_fsr2_api_x64.dll")                                                                   \
    X(Fsr2ContextDestroy, "fsr2", "ffxFsr2ContextDestroy", "ffx_fsr2_api_x64.dll")              \
    X(Fsr2GetJitterOffset, "fsr2", "ffxFsr2GetJitterOffset", "ffx_fsr2_api_x64.dll")            \
    X(Fsr2GetJitterPhaseCount, "fsr2", "ffxFsr2GetJitterPhaseCount", "ffx_fsr2_api_x64.dll")    \
    X(Fsr2GetRenderResolution, "fsr2", "ffxFsr2GetRenderResolutionFromQualityMode",             \
      "ffx_fsr2_api_x64.dll")                                                                   \
    X(Fsr2GetUpscaleRatio, "fsr2", "ffxFsr2GetUpscaleRatioFromQualityMode",                     \
      "ffx_fsr2_api_x64.dll")                                                                   \
    X(Fsr2GetInterfaceDX12, "fsr2", "ffxFsr2GetInterfaceDX12", "ffx_fsr2_api_dx12_x64.dll")     \
    X(Fsr2GetScratchMemorySizeDX12, "fsr2", "ffxFsr2GetScratchMemorySizeDX12",                  \
      "ffx_fsr2_api_dx12_x64.dll")                                                              \
    X(FfxGetResourceDX12, "fsr2", "ffxGetResourceDX12", "ffx_fsr2_api_dx12_x64.dll")            \
    X(FfxGetCommandListDX12, "fsr2", "ffxGetCommandListDX12", "ffx_fsr2_api_dx12_x64.dll")      \
    X(FfxGetDeviceDX12, "fsr2", "ffxGetDeviceDX12", "ffx_fsr2_api_dx12_x64.dll")                \
    X(FfxCreateContext, "ffx_api", "ffxCreateContext", RSF_FFX_API_MODULES)                     \
    X(FfxDestroyContext, "ffx_api", "ffxDestroyContext", RSF_FFX_API_MODULES)                   \
    X(FfxConfigure, "ffx_api", "ffxConfigure", RSF_FFX_API_MODULES)                             \
    X(FfxQuery, "ffx_api", "ffxQuery", RSF_FFX_API_MODULES)                                     \
    X(FfxDispatch, "ffx_api", "ffxDispatch", RSF_FFX_API_MODULES)                               \
    X(XessD3D12CreateContext, "xess", "xessD3D12CreateContext", RSF_XESS_MODULES)               \
    X(XessD3D12Init, "xess", "xessD3D12Init", RSF_XESS_MODULES)                                 \
    X(XessD3D12Execute, "xess", "xessD3D12Execute", RSF_XESS_MODULES)                           \
    X(XessD3D11CreateContext, "xess", "xessD3D11CreateContext", RSF_XESS_MODULES)               \
    X(XessD3D11Init, "xess", "xessD3D11Init", RSF_XESS_MODULES)                                 \
    X(XessD3D11Execute, "xess", "xessD3D11Execute", RSF_XESS_MODULES)                           \
    X(XessVKCreateContext, "xess", "xessVKCreateContext", RSF_XESS_MODULES)                     \
    X(XessVKInit, "xess", "xessVKInit", RSF_XESS_MODULES)                                       \
    X(XessVKExecute, "xess", "xessVKExecute", RSF_XESS_MODULES)                                 \
    X(XessSetJitterScale, "xess", "xessSetJitterScale", RSF_XESS_MODULES)                       \
    X(XessSetVelocityScale, "xess", "xessSetVelocityScale", RSF_XESS_MODULES)                   \
    X(XessSetExposureMultiplier, "xess", "xessSetExposureMultiplier", RSF_XESS_MODULES)         \
    X(XessDestroyContext, "xess", "xessDestroyContext", RSF_XESS_MODULES)                       \
    X(XefgCreateContext, "xefg", "xefgSwapChainD3D12CreateContext", "libxess_fg.dll")           \
    X(XefgInitFromSwapChain, "xefg", "xefgSwapChainD3D12InitFromSwapChain", "libxess_fg.dll")   \
    X(XefgInitFromSwapChainDesc, "xefg", "xefgSwapChainD3D12InitFromSwapChainDesc",             \
      "libxess_fg.dll")                                                                         \
    X(XefgTagFrameResource, "xefg", "xefgSwapChainD3D12TagFrameResource", "libxess_fg.dll")     \
    X(XefgTagFrameConstants, "xefg", "xefgSwapChainTagFrameConstants", "libxess_fg.dll")        \
    X(XefgSetEnabled, "xefg", "xefgSwapChainSetEnabled", "libxess_fg.dll")                      \
    X(XefgSetPresentId, "xefg", "xefgSwapChainSetPresentId", "libxess_fg.dll")                  \
    X(XefgSetLatencyReduction, "xefg", "xefgSwapChainSetLatencyReduction", "libxess_fg.dll")    \
    X(XefgSetNumInterpolatedFrames, "xefg", "xefgSwapChainSetNumInterpolatedFrames",            \
      "libxess_fg.dll")                                                                         \
    X(XefgSetUiCompositionState, "xefg", "xefgSwapChainSetUiCompositionState",                  \
      "libxess_fg.dll")                                                                         \
    X(XefgDestroy, "xefg", "xefgSwapChainDestroy", "libxess_fg.dll")                            \
    X(XellD3D12CreateContext, "xell", "xellD3D12CreateContext", "libxell.dll")                  \
    X(XellDestroyContext, "xell", "xellDestroyContext", "libxell.dll")                          \
    X(XellSetSleepMode, "xell", "xellSetSleepMode", "libxell.dll")                              \
    X(XellGetSleepMode, "xell", "xellGetSleepMode", "libxell.dll")                              \
    X(XellSleep, "xell", "xellSleep", "libxell.dll")                                            \
    X(XellAddMarkerData, "xell", "xellAddMarkerData", "libxell.dll")                            \
    X(SlInit, "streamline", "slInit", "sl.interposer.dll")                                      \
    X(SlShutdown, "streamline", "slShutdown", "sl.interposer.dll")                              \
    X(SlSetTag, "streamline", "slSetTag", "sl.interposer.dll")                                  \
    X(SlSetTagForFrame, "streamline", "slSetTagForFrame", "sl.interposer.dll")                  \
    X(SlSetConstants, "streamline", "slSetConstants", "sl.interposer.dll")                      \
    X(SlEvaluateFeature, "streamline", "slEvaluateFeature", "sl.interposer.dll")                \
    X(SlGetNewFrameToken, "streamline", "slGetNewFrameToken", "sl.interposer.dll")              \
    X(SlGetFeatureFunction, "streamline", "slGetFeatureFunction", "sl.interposer.dll")          \
    X(SlIsFeatureSupported, "streamline", "slIsFeatureSupported", "sl.interposer.dll")          \
    X(SlSetD3DDevice, "streamline", "slSetD3DDevice", "sl.interposer.dll")                      \
    X(SlUpgradeInterface, "streamline", "slUpgradeInterface", "sl.interposer.dll")              \
    X(SlAllocateResources, "streamline", "slAllocateResources", "sl.interposer.dll")            \
    X(SlFreeResources, "streamline", "slFreeResources", "sl.interposer.dll")                    \
    X(SlDLSSGSetOptions, "streamline", "slDLSSGSetOptions", "sl.interposer.dll")                \
    X(SlDLSSGGetState, "streamline", "slDLSSGGetState", "sl.interposer.dll")                    \
    X(SlDLSSSetOptions, "streamline", "slDLSSSetOptions", "sl.interposer.dll")                  \
    X(SlReflexSleep, "streamline", "slReflexSleep", "sl.interposer.dll")                        \
    X(SlReflexSetOptions, "streamline", "slReflexSetOptions", "sl.interposer.dll")              \
    X(SlPclSetMarker, "streamline", "slPCLSetMarker", "sl.interposer.dll")                      \
    X(SlPclSetOptions, "streamline", "slPCLSetOptions", "sl.interposer.dll")                    \
    X(NvapiQueryInterface, "nvapi", "nvapi_QueryInterface", "nvapi64.dll")                      \
    X(NvapiSetSleepMode, "nvapi_reflex", nullptr, "")                                           \
    X(NvapiSleep, "nvapi_reflex", nullptr, "")                                                  \
    X(NvapiSetLatencyMarker, "nvapi_reflex", nullptr, "")                                       \
    X(NvapiGetLatency, "nvapi_reflex", nullptr, "")                                             \
    X(NvapiD3D12SetAsyncFrameMarker, "nvapi_reflex", nullptr, "")                               \
    X(D3d12DebugMessage, "d3d12_debug", nullptr, "")                                            \
    X(CensusEvent, "census", nullptr, "")                                                       \
    X(NvllInitialize, "nvll_vk", "NvLL_VK_Initialize", "nvlowlatencyvk.dll")                    \
    X(NvllInitLowLatencyDevice, "nvll_vk", "NvLL_VK_InitLowLatencyDevice",                      \
      "nvlowlatencyvk.dll")                                                                     \
    X(NvllDestroyLowLatencyDevice, "nvll_vk", "NvLL_VK_DestroyLowLatencyDevice",                \
      "nvlowlatencyvk.dll")                                                                     \
    X(NvllSetSleepMode, "nvll_vk", "NvLL_VK_SetSleepMode", "nvlowlatencyvk.dll")                \
    X(NvllSleep, "nvll_vk", "NvLL_VK_Sleep", "nvlowlatencyvk.dll")                              \
    X(NvllSetLatencyMarker, "nvll_vk", "NvLL_VK_SetLatencyMarker", "nvlowlatencyvk.dll")        \
    X(NvllGetLatency, "nvll_vk", "NvLL_VK_GetLatency", "nvlowlatencyvk.dll")                    \
    X(AmdExtD3DCreateInterface, "amd_antilag", "AmdExtD3DCreateInterface", "amdxc64.dll")       \
    X(AgsInitialize, "amd_ags", "agsInitialize", "amd_ags_x64.dll")                             \
    X(AgsDriverExtensionsDX12CreateDevice, "amd_ags", "agsDriverExtensionsDX12_CreateDevice",   \
      "amd_ags_x64.dll")

#define RSF_FFX_API_MODULES                                                                      \
    "amd_fidelityfx_dx12.dll|amd_fidelityfx_vk.dll|amd_fidelityfx_loader_dx12.dll|"             \
    "amd_fidelityfx_upscaler_dx12.dll|amd_fidelityfx_framegeneration_dx12.dll"
#define RSF_XESS_MODULES "libxess.dll|libxess_dx11.dll"

enum class Fn : uint16_t {
#define X(id, api, name, mods) id,
    RSF_CENSUS_FUNCTIONS(X)
#undef X
        Count
};

struct FnDesc {
    const char* id;
    const char* api;
    const char* name;    // export name, may be null
    const char* modules; // see above
};

constexpr size_t kFnCount = static_cast<size_t>(Fn::Count);
extern const FnDesc kFns[kFnCount];
inline size_t idx(Fn f) noexcept { return static_cast<size_t>(f); }

// ---- Bound originals --------------------------------------------------------------------------
// g_orig is the function a wrapper forwards to: a MinHook trampoline for a detour, or the real
// export for a GetProcAddress thunk. g_wrapper is the wrapper's address. g_target is the detoured
// function for an inline binding and null for a thunk binding.
extern std::atomic<void*> g_orig[kFnCount];
extern void* g_wrapper[kFnCount];
extern std::atomic<void*> g_target[kFnCount];

template <Fn F>
inline void* orig() noexcept {
    return g_orig[idx(F)].load(std::memory_order_acquire);
}
inline void* orig_by(Fn f) noexcept { return g_orig[idx(f)].load(std::memory_order_acquire); }

// First binding wins. Returns the address to hand back to the game: the wrapper, or `real` when
// the entry is bound to a different implementation (counted as a conflict) or already detoured.
void* bind_thunk(Fn fn, void* real) noexcept;
// Bind the address a vtable slot points at with an inline detour. Returns false when the entry is
// already bound or the detour fails.
bool bind_inline(Fn fn, void* target, const char* why) noexcept;
// Apply the detours queued by bind_inline in one thread freeze.
void hooks_apply() noexcept;
// Look up an export or API name in the function table. Returns Fn::Count when unknown.
Fn lookup_name(const char* name) noexcept;

// ---- Safe memory access -----------------------------------------------------------------------
bool safe_read(const void* src, void* dst, size_t n) noexcept;
bool safe_read_cstr(const char* src, char* dst, size_t cap) noexcept;
bool safe_read_wstr(const wchar_t* src, wchar_t* dst, size_t cap) noexcept;
bool plausible_ptr(const void* p) noexcept;

template <class T>
inline bool rd(const void* p, T& out) noexcept {
    return safe_read(p, &out, sizeof(T));
}
template <class T>
inline T rd_or(const void* p, T fallback) noexcept {
    T v{};
    return safe_read(p, &v, sizeof(T)) ? v : fallback;
}
// True when `p` is committed executable memory.
bool is_code_ptr(const void* p) noexcept;
// Read a vtable entry: `obj` points at an object whose first word is the vtable. Returns null
// unless the entry points at executable memory, so a bad object is never called.
void* vtable_slot(const void* obj, size_t slot) noexcept;

// Run `fn(ctx)` under SEH. Returns false when it faulted.
bool guarded(void (*fn)(void*), void* ctx) noexcept;
template <class L>
inline bool guard(L&& lambda) noexcept {
    using T = std::remove_reference_t<L>;
    return guarded([](void* c) { (*static_cast<T*>(c))(); }, &lambda);
}

// ---- JSON line builder ------------------------------------------------------------------------
class Json {
public:
    Json(char* buf, size_t cap) noexcept : buf_(buf), cap_(cap) {}
    bool ok() const noexcept { return !overflow_; }
    size_t size() const noexcept { return len_; }

    void obj(const char* key = nullptr) noexcept { open(key, '{'); }
    void end_obj() noexcept { close('}'); }
    void arr(const char* key = nullptr) noexcept { open(key, '['); }
    void end_arr() noexcept { close(']'); }
    void str(const char* key, const char* v) noexcept;
    void wstr(const char* key, const wchar_t* v) noexcept;
    void u64(const char* key, uint64_t v) noexcept;
    void i64(const char* key, int64_t v) noexcept;
    void hex(const char* key, uint64_t v) noexcept; // "0x..." string
    void f32(const char* key, double v) noexcept;
    void boolean(const char* key, bool v) noexcept;
    void null(const char* key) noexcept;
    void bytes_hex(const char* key, const void* data, size_t n) noexcept;
    void raw(const char* key, const char* json_text) noexcept;

private:
    void open(const char* key, char c) noexcept;
    void close(char c) noexcept;
    void lead(const char* key) noexcept;
    void put(const char* s, size_t n) noexcept;
    void put(const char* s) noexcept { put(s, std::strlen(s)); }
    void put_escaped(const char* s) noexcept;

    char* buf_;
    size_t cap_;
    size_t len_ = 0;
    uint32_t comma_ = 0; // bit per nesting level: an element already written
    uint32_t depth_ = 0;
    bool overflow_ = false;
};

// ---- Per-call recording -----------------------------------------------------------------------
constexpr size_t kBodyMax = 6144;
// D3D12 validation can consume more than 16 frames before reaching the game's recording call.
constexpr uint32_t kStackFrames = 32;

class Call {
public:
    Call(Fn fn, void* return_address) noexcept;
    Call(const Call&) = delete;
    Call& operator=(const Call&) = delete;
    // True when this call is written in full. Decode arguments only then.
    bool full() const noexcept { return full_; }
    Json& json() noexcept { return json_; }
    // Queue the record. `ret` is the raw return register.
    void finish(uint64_t ret) noexcept;

private:
    Fn fn_;
    bool full_;
    uint32_t rva_ = 0;
    uint32_t tid_ = 0;
    uint64_t base_ = 0;
    uint64_t site_ord_ = 0;
    uint64_t ord_ = 0;
    uint64_t t_us_ = 0;
    // Return addresses from the intercepted call's caller outward, captured for full records only.
    void* stack_[kStackFrames] = {};
    uint32_t stack_n_ = 0;
    Json json_;
    char buf_[kBodyMax];
};

#define RSF_RETADDR() _ReturnAddress()

// Present boundary: call before the real Present, before opening the Call for it.
void note_present_begin() noexcept;
uint64_t present_index() noexcept;

enum class QueueKind : uint8_t { D3d12 = 1, Vulkan = 2 };
// Describe a queue once (D3D12: type, priority, flags; Vulkan: family, index, unused).
void queue_describe(QueueKind kind, const void* queue, uint32_t a, uint32_t b, uint32_t c) noexcept;
// Count one submission call carrying `lists` command lists or batches. Returns true the first
// time a queue is seen, so the caller can describe a queue that predates the census.
bool queue_submit(QueueKind kind, const void* queue, uint32_t lists) noexcept;

// Present calls can nest (a proxy swapchain calling the one beneath it). Only the outermost counts.
inline thread_local int tl_present_depth = 0;
struct PresentScope {
    PresentScope() noexcept : outer(tl_present_depth++ == 0) {}
    ~PresentScope() noexcept { --tl_present_depth; }
    PresentScope(const PresentScope&) = delete;
    PresentScope& operator=(const PresentScope&) = delete;
    bool outer;
};

// Static findings (statically linked vendor code), kept for the summary.
void add_finding(const char* vendor, const char* what, uint64_t rva, uint64_t function_rva) noexcept;

bool installed() noexcept;
uint32_t option_flags() noexcept;

// Hook installation entry points of each translation unit.
void register_wrappers_d3d(void** table) noexcept;
void register_wrappers_vk(void** table) noexcept;
void register_wrappers_ngx(void** table) noexcept;
void register_wrappers_fsr(void** table) noexcept;
void register_wrappers_misc(void** table) noexcept;

// Hook layer (hooks.cpp).
bool hooks_install() noexcept;
void hooks_remove() noexcept;
void hook_counts(uint32_t& installed, uint32_t& conflicts, uint32_t& modules) noexcept;
// Summary contributions written by the hook layer.
void hooks_summary(Json& j) noexcept;

// Core services (core.cpp).
rsf_census_result core_install(const rsf_census_options* opt) noexcept;
void static_scan_run() noexcept;

// Resource decode helpers (decode.cpp).
const char* dxgi_format_name(uint32_t format) noexcept;
void json_d3d12_resource(Json& j, const char* key, const void* resource) noexcept;
void json_d3d11_resource(Json& j, const char* key, const void* resource) noexcept;
void json_ptr_or_null(Json& j, const char* key, const void* p) noexcept;
// Read a narrow or wide string argument; returns false when unreadable.
bool read_arg_string(const void* p, char* out, size_t cap) noexcept;
bool read_arg_wstring(const void* p, wchar_t* out, size_t cap) noexcept;

} // namespace rsf_census
