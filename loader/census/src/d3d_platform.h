/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_D3D_PLATFORM_H
#define RSF_CENSUS_D3D_PLATFORM_H

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

#endif
