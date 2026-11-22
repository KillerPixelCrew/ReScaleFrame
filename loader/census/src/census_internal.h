// SPDX-License-Identifier: MIT
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
#include "decode_platform.h"
#include "hook_platform.h"
#include "record_platform.h"
#include "session_platform.h"
#include "stats_platform.h"

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

using FnDesc = rsf_census_hook_desc;

constexpr size_t kFnCount = static_cast<size_t>(Fn::Count);
extern const FnDesc kFns[kFnCount];

// ---- Safe memory access -----------------------------------------------------------------------
bool safe_read(const void* src, void* dst, size_t n) noexcept;
bool safe_read_cstr(const char* src, char* dst, size_t cap) noexcept;
bool safe_read_wstr(const wchar_t* src, wchar_t* dst, size_t cap) noexcept;
bool plausible_ptr(const void* p) noexcept;

template <class T>
inline bool rd(const void* p, T& out) noexcept {
    return safe_read(p, &out, sizeof(T));
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

// Hook installation entry points of each translation unit.
void register_wrappers_d3d(void** table) noexcept;

extern const rsf_census_hook_platform kHookPlatform;

void describe_d3d_resource(rsf_census_json* json, const char* key, const void* resource,
                           bool d3d12) noexcept;
// Read a narrow or wide string argument; returns false when unreadable.
inline bool read_arg_string(const void* p, char* out, size_t cap) noexcept {
    if (!cap) return false;
    out[0] = 0;
    return plausible_ptr(p) && safe_read_cstr(static_cast<const char*>(p), out, cap);
}
inline bool read_arg_wstring(const void* p, wchar_t* out, size_t cap) noexcept {
    if (!cap) return false;
    out[0] = 0;
    return plausible_ptr(p) && safe_read_wstr(static_cast<const wchar_t*>(p), out, cap);
}

} // namespace rsf_census
