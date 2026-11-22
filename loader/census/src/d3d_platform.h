/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_D3D_PLATFORM_H
#define RSF_CENSUS_D3D_PLATFORM_H

#include "json_platform.h"

#define RSF_CENSUS_D3D_ENTRIES(X) \
    X(CreateDXGIFactory) X(CreateDXGIFactory1) X(CreateDXGIFactory2) X(D3D12CreateDevice) \
    X(DxgiFactoryCreateSwapChain) X(DxgiFactoryCreateSwapChainForHwnd) \
    X(DxgiFactoryCreateSwapChainForCoreWindow) X(DxgiSwapChainPresent) X(DxgiSwapChainPresent1) \
    X(DxgiSwapChainResizeBuffers) X(DxgiSwapChainResizeBuffers1) X(DxgiSwapChainSetColorSpace1) \
    X(DxgiSwapChainSetFullscreenState) X(DxgiSwapChainGetFrameLatencyWaitableObject) \
    X(D3d12DeviceCreateCommandQueue) X(D3d12QueueExecuteCommandLists) X(D3d12DebugMessage) X(CensusEvent)

typedef enum rsf_census_d3d_entry {
#define RSF_CENSUS_D3D_VALUE(name) RSF_D3D_##name,
    RSF_CENSUS_D3D_ENTRIES(RSF_CENSUS_D3D_VALUE)
#undef RSF_CENSUS_D3D_VALUE
    RSF_D3D_COUNT
} rsf_census_d3d_entry;

typedef void (*rsf_census_debug_message)(int32_t, int32_t, int32_t, const char*, void*);

typedef struct rsf_census_d3d_platform {
    bool (*read)(const void*, void*, size_t);
    bool (*read_name)(const char*, char*, size_t);
    void* (*slot)(const void*, size_t);
    void* (*query)(void* object, const void* iid);
    void (*release)(void* object);
    bool (*queue_desc)(const void*, void* desc);
    bool (*swapchain_desc)(const void*, void* desc);
    void (*debug_layer)(int32_t* layer, int32_t* dred);
    int32_t (*info_queue)(void* device, rsf_census_debug_message, uint32_t* cookie);
    bool (*present_enter)(bool test);
    void (*present_leave)(void);
} rsf_census_d3d_platform;

#ifdef __cplusplus
extern "C" {
#endif
void rsf_census_d3d_bind(const rsf_census_d3d_platform*, const uint16_t* entries, void** wrappers);
#ifdef __cplusplus
}
#endif
#endif
