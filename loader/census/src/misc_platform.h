/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_MISC_PLATFORM_H
#define RSF_CENSUS_MISC_PLATFORM_H

#define RSF_CENSUS_MISC_ENTRIES(X) \
    X(NvapiQueryInterface) X(NvapiSetSleepMode) X(NvapiSleep) X(NvapiSetLatencyMarker) \
    X(NvapiGetLatency) X(NvapiD3D12SetAsyncFrameMarker) X(NvllInitialize) \
    X(NvllInitLowLatencyDevice) X(NvllDestroyLowLatencyDevice) X(NvllSetSleepMode) \
    X(NvllSleep) X(NvllSetLatencyMarker) X(NvllGetLatency) X(SlInit) X(SlShutdown) X(SlSetTag) \
    X(SlSetTagForFrame) X(SlSetConstants) X(SlEvaluateFeature) X(SlGetNewFrameToken) X(SlGetFeatureFunction) \
    X(SlIsFeatureSupported) X(SlSetD3DDevice) X(SlUpgradeInterface) X(SlAllocateResources) \
    X(SlFreeResources) X(SlDLSSGSetOptions) X(SlDLSSGGetState) X(SlDLSSSetOptions) X(SlReflexSleep) \
    X(SlReflexSetOptions) X(SlPclSetMarker) X(SlPclSetOptions) X(AmdExtD3DCreateInterface) \
    X(AgsInitialize) X(AgsDriverExtensionsDX12CreateDevice)

typedef enum rsf_census_misc_entry {
#define RSF_CENSUS_MISC_VALUE(name) RSF_MISC_##name,
    RSF_CENSUS_MISC_ENTRIES(RSF_CENSUS_MISC_VALUE)
#undef RSF_CENSUS_MISC_VALUE
    RSF_MISC_COUNT
} rsf_census_misc_entry;

#endif
