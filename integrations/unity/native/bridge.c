/* SPDX-License-Identifier: MIT */
/* Pinned Unity SDK renderer query and the D3D11 interface for the package bridge. */
#include "bridge.h"
#include <stdbool.h>
#include <windows.h>
#include <d3d11.h>
#define RSF_UNITY_SDK_LEAF rsf_unity_package_bridge
#include "unity_sdk.h"
#include <IUnityGraphicsD3D11.h>

static IUnityGraphicsD3D11* unity = NULL;

uint32_t rsf_unity_package_sdk_renderer(void* pointer, void** graphics_interface)
{
    IUnityInterfaces* interfaces = (IUnityInterfaces*)pointer;
    IUnityGraphics* graphics = RSF_UNITY_GET(interfaces, IUnityGraphics);
    *graphics_interface = graphics;
    if (!graphics) return RSF_UNITY_BRIDGE_NONE;
    switch (graphics->GetRenderer()) {
    case kUnityGfxRendererD3D11: return RSF_UNITY_BRIDGE_D3D11;
    case kUnityGfxRendererD3D12: return RSF_UNITY_BRIDGE_D3D12;
    case kUnityGfxRendererVulkan: return RSF_UNITY_BRIDGE_VULKAN;
    default: return RSF_UNITY_BRIDGE_NONE;
    }
}

int rsf_unity_package_sdk_reserve_events(void* graphics)
{
    return ((IUnityGraphics*)graphics)->ReserveEventIDRange(3);
}

void rsf_unity_package_sdk_bind_d3d11(void* interfaces)
{
    unity = RSF_UNITY_GET((IUnityInterfaces*)interfaces, IUnityGraphicsD3D11);
}

void rsf_unity_package_sdk_unbind_d3d11(void) { unity = NULL; }
bool rsf_unity_package_sdk_has_d3d11(void) { return unity != NULL; }
void* rsf_unity_d3d11_engine_device(void) { return unity ? unity->GetDevice() : NULL; }
/* The public NativeRenderingPlugin pin has no D3D11 swapchain getter; some provisioned headers
   declare it. CMake defines RSF_UNITY_D3D11_SWAPCHAIN only when the supplied header names the
   member, so no private layout is assumed. */
void* rsf_unity_d3d11_engine_swapchain(void)
{
#if RSF_UNITY_D3D11_SWAPCHAIN
    return unity ? unity->GetSwapChain() : NULL;
#else
    return NULL;
#endif
}
