/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_FSR_PLATFORM_H
#define RSF_CENSUS_FSR_PLATFORM_H

#include "json_platform.h"

#define RSF_CENSUS_FSR_ENTRIES(X) \
    X(Fsr2ContextCreate) X(Fsr2ContextDispatch) X(Fsr2ContextGenerateReactiveMask) \
    X(Fsr2ContextDestroy) X(Fsr2GetJitterOffset) X(Fsr2GetJitterPhaseCount) \
    X(Fsr2GetRenderResolution) X(Fsr2GetUpscaleRatio) X(Fsr2GetInterfaceDX12) \
    X(Fsr2GetScratchMemorySizeDX12) X(FfxGetResourceDX12) X(FfxGetCommandListDX12) \
    X(FfxGetDeviceDX12) X(FfxCreateContext) X(FfxDestroyContext) \
    X(FfxConfigure) X(FfxQuery) X(FfxDispatch) \
    X(XessD3D12CreateContext) X(XessD3D12Init) X(XessD3D12Execute) \
    X(XessD3D11CreateContext) X(XessD3D11Init) X(XessD3D11Execute) \
    X(XessVKCreateContext) X(XessVKInit) X(XessVKExecute) \
    X(XessSetJitterScale) X(XessSetVelocityScale) X(XessSetExposureMultiplier) \
    X(XessDestroyContext) X(XefgCreateContext) X(XefgInitFromSwapChain) \
    X(XefgInitFromSwapChainDesc) X(XefgTagFrameResource) X(XefgTagFrameConstants) \
    X(XefgSetEnabled) X(XefgSetPresentId) X(XefgSetLatencyReduction) \
    X(XefgSetNumInterpolatedFrames) X(XefgSetUiCompositionState) X(XefgDestroy) \
    X(XellD3D12CreateContext) X(XellDestroyContext) X(XellSetSleepMode) \
    X(XellGetSleepMode) X(XellSleep) X(XellAddMarkerData)

typedef enum rsf_census_fsr_entry {
#define RSF_CENSUS_FSR_VALUE(name) RSF_FSR_##name,
    RSF_CENSUS_FSR_ENTRIES(RSF_CENSUS_FSR_VALUE)
#undef RSF_CENSUS_FSR_VALUE
    RSF_FSR_COUNT
} rsf_census_fsr_entry;

typedef struct rsf_census_fsr_platform {
    bool (*read)(const void*, void*, size_t);
    void* (*slot)(const void*, size_t);
    void (*resource)(rsf_census_json*, const char* key, const void*);
    void (*decimal)(rsf_census_json*, const char* key, double value);
} rsf_census_fsr_platform;

#ifdef __cplusplus
extern "C" {
#endif
void rsf_census_fsr_bind(const rsf_census_fsr_platform*, const uint16_t* entries, void** wrappers);
#ifdef __cplusplus
}
#endif
#endif
