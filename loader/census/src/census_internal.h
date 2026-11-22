/* SPDX-License-Identifier: MIT */
/* Native declarations shared by the census platform leaves. Nothing here crosses the DLL boundary. */
#ifndef RSF_CENSUS_INTERNAL_H
#define RSF_CENSUS_INTERNAL_H

#include <rescaleframe/foreign_guard.h>
#include <windows.h>

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "rescaleframe/vendor_census.h"
#include "census_platform.h"

/* ---- Intercepted functions ------------------------------------------------------------------ */
/* X(id, api, export name or NULL, modules). `modules` is a '|' separated list of lowercase
   module basenames whose exports are detoured when the module loads. An entry with no export name
   is bound by the DXGI and Vulkan code that learns the address at run time. */
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
    X(DxgiFactoryCreateSwapChain, "dxgi", NULL, "")                                          \
    X(DxgiFactoryCreateSwapChainForHwnd, "dxgi", NULL, "")                                   \
    X(DxgiFactoryCreateSwapChainForCoreWindow, "dxgi", NULL, "")                             \
    X(DxgiSwapChainPresent, "dxgi", NULL, "")                                                \
    X(DxgiSwapChainPresent1, "dxgi", NULL, "")                                               \
    X(DxgiSwapChainResizeBuffers, "dxgi", NULL, "")                                          \
    X(DxgiSwapChainResizeBuffers1, "dxgi", NULL, "")                                         \
    X(DxgiSwapChainSetColorSpace1, "dxgi", NULL, "")                                         \
    X(DxgiSwapChainSetFullscreenState, "dxgi", NULL, "")                                     \
    X(DxgiSwapChainGetFrameLatencyWaitableObject, "dxgi", NULL, "")                          \
    X(D3d12DeviceCreateCommandQueue, "d3d12", NULL, "")                                      \
    X(D3d12QueueExecuteCommandLists, "d3d12", NULL, "")                                      \
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
    X(NvapiSetSleepMode, "nvapi_reflex", NULL, "")                                           \
    X(NvapiSleep, "nvapi_reflex", NULL, "")                                                  \
    X(NvapiSetLatencyMarker, "nvapi_reflex", NULL, "")                                       \
    X(NvapiGetLatency, "nvapi_reflex", NULL, "")                                             \
    X(NvapiD3D12SetAsyncFrameMarker, "nvapi_reflex", NULL, "")                               \
    X(D3d12DebugMessage, "d3d12_debug", NULL, "")                                            \
    X(CensusEvent, "census", NULL, "")                                                       \
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

typedef enum rsf_census_fn {
#define X(id, api, name, mods) RSF_FN_##id,
    RSF_CENSUS_FUNCTIONS(X)
#undef X
    RSF_FN_COUNT
} rsf_census_fn;

extern const rsf_census_hook_desc rsf_census_functions[RSF_FN_COUNT];

/* ---- Safe memory access --------------------------------------------------------------------- */
bool rsf_census_safe_read(const void* src, void* dst, size_t n);
bool rsf_census_safe_read_cstr(const char* src, char* dst, size_t cap);
bool rsf_census_safe_read_wstr(const wchar_t* src, wchar_t* dst, size_t cap);
bool rsf_census_plausible_ptr(const void* p);
/* True when `p` is committed executable memory. */
bool rsf_census_is_code_ptr(const void* p);
/* Read a vtable entry: `obj` points at an object whose first word is the vtable. Returns NULL
   unless the entry points at executable memory, so a bad object is never called. */
void* rsf_census_vtable_slot(const void* obj, size_t slot);
/* Read a narrow or wide string argument; returns false when unreadable. */
bool rsf_census_read_arg_string(const void* p, char* out, size_t cap);
bool rsf_census_read_arg_wstring(const void* p, wchar_t* out, size_t cap);

/* SEH-guarded calls through D3D12, D3D11 and DXGI vtables (d3d_guarded_platform.c). */
int32_t rsf_census_query_interface(void* object, const void* iid, void** result);
void rsf_census_release_interface(void* object);
bool rsf_census_queue_desc(const void* queue, void* description);
bool rsf_census_swapchain_desc(const void* chain, void* description);
bool rsf_census_resource12_desc(const void* resource, void* description);
bool rsf_census_resource11_dimension(const void* resource, int32_t* dimension);
bool rsf_census_resource11_desc(const void* resource, void* description);
#endif