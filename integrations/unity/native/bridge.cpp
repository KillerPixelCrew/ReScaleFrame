// SPDX-License-Identifier: MIT
// Pinned Unity SDK binding and the existing U04 native Vulkan boundary.
#include "bridge.h"
#include "vulkan_bridge.h"
#include <windows.h>
#include <d3d11.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <IUnityGraphics.h>
#include <IUnityGraphicsD3D11.h>

namespace {
IUnityGraphicsD3D11* unity = nullptr;

// The public pin has no swapchain getter. Some provisioned headers declare it.
// Use the member only when the supplied SDK names it, without assuming a private layout.
template <class T>
auto swapchain(T* sdk, int) -> decltype(sdk->GetSwapChain()) { return sdk->GetSwapChain(); }
template <class T>
void* swapchain(T*, ...) { return nullptr; }
}

extern "C" uint32_t rsf_unity_package_sdk_renderer(void* pointer, void** graphics_interface)
{
    auto* interfaces = static_cast<IUnityInterfaces*>(pointer);
    auto* graphics = interfaces->Get<IUnityGraphics>();
    *graphics_interface = graphics;
    if (!graphics) return RSF_UNITY_BRIDGE_NONE;
    switch (graphics->GetRenderer()) {
    case kUnityGfxRendererD3D11: return RSF_UNITY_BRIDGE_D3D11;
    case kUnityGfxRendererD3D12: return RSF_UNITY_BRIDGE_D3D12;
    case kUnityGfxRendererVulkan: return RSF_UNITY_BRIDGE_VULKAN;
    default: return RSF_UNITY_BRIDGE_NONE;
    }
}
extern "C" int rsf_unity_package_sdk_reserve_events(void* graphics)
{
    return static_cast<IUnityGraphics*>(graphics)->ReserveEventIDRange(3);
}
extern "C" void rsf_unity_package_sdk_bind_d3d11(void* interfaces)
{
    unity = static_cast<IUnityInterfaces*>(interfaces)->Get<IUnityGraphicsD3D11>();
}
extern "C" void rsf_unity_package_sdk_unbind_d3d11(void) { unity = nullptr; }
extern "C" bool rsf_unity_package_sdk_has_d3d11(void) { return unity != nullptr; }
extern "C" void* rsf_unity_d3d11_engine_device(void) { return unity ? unity->GetDevice() : nullptr; }
extern "C" void* rsf_unity_d3d11_engine_swapchain(void) { return swapchain(unity, 0); }

extern "C" void rsf_unity_package_sdk_bind_vulkan(void* interfaces, int event)
{
    rsf_uvk_load(static_cast<IUnityInterfaces*>(interfaces), event);
}
extern "C" void rsf_unity_package_sdk_unbind_vulkan(void) { rsf_uvk_unload(); }

extern "C" bool rsf_unity_package_vulkan_create_window(void) try
{
    return rsf_uvk_create_window();
} catch (...) { return false; }

extern "C" int rsf_unity_package_vulkan_render(int id, const rsf_unity_packet* packet,
    const rsf_game_host_services* services, bool reconfigure) try
{
    return rsf_uvk_render(id, *packet, *services, reconfigure) ? 1 : 0;
} catch (...) { return -1; }

// The U04 API already takes ownership under Vulkan admission. Adapt its ComPtr outputs.
extern "C" bool rsf_unity_package_vulkan_graphics(void** device, void** queue)
{
    Microsoft::WRL::ComPtr<ID3D12Device> owned_device;
    Microsoft::WRL::ComPtr<ID3D12CommandQueue> owned_queue;
    if (!rsf_uvk_graphics(owned_device, owned_queue) || !owned_device || !owned_queue) return false;
    *device = owned_device.Detach();
    *queue = owned_queue.Detach();
    return true;
}
