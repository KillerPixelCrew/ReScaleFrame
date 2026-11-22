// SPDX-License-Identifier: MIT
// Unity package SDK registration and routing for the shared graphics bridges.
#include "bridge.h"
#include "d3d11_state.h"
#include "vulkan_bridge.h"
#include "vulkan_state.h"
#include "package.h"
#include "preview.h"
#include <windows.h>
#include <d3d11.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <IUnityGraphics.h>
#include <IUnityGraphicsD3D11.h>
#include <IUnityGraphicsD3D12.h>
#include <rescaleframe/d3d11_present_bridge.h>
#include <mutex>

extern bool rsf_unity_d3d12_prepare(const rsf_game_host_services&, rsf_unity_native_api*) noexcept;
extern void rsf_unity_d3d12_activate(bool) noexcept;
extern bool rsf_unity_d3d12_try_close() noexcept;
extern void rsf_unity_d3d12_release() noexcept;
extern uint32_t rsf_unity_d3d12_managed_state() noexcept;
extern "C" void UNITY_INTERFACE_API rsf_unity_d3d12_load(IUnityInterfaces*);
extern "C" void UNITY_INTERFACE_API rsf_unity_d3d12_unload();
extern "C" void* rsf_unity_d3d12_get_render_event() noexcept;
extern "C" int rsf_unity_d3d12_get_event_id() noexcept;
extern "C" int rsf_unity_d3d12_set_engine_spatial(uint32_t, uint32_t) noexcept;

namespace {
IUnityGraphicsD3D11* unity11 = nullptr;
IUnityGraphicsD3D12v7* unity12 = nullptr;
UnityGfxRenderer renderer = kUnityGfxRendererNull;
// Guards the host services. Enqueue reads the session under it.
std::mutex guard;
rsf_game_host_services services{};
int event_id = 0;
// Vulkan packets carry image handles and enter the store without COM leases.
constexpr rsf_unity_texture_ops texture_ops{rsf_unity_retain_texture, rsf_unity_release_texture};
bool active() { return rsf_unity_frames_active() != 0; }
void log(const char* text) { if (services.log) services.log(services.user, text); }
void UNITY_INTERFACE_API render_event(int id, void* address) noexcept try
{
    rsf_unity_frame_callback call;
    if (!call.entered) return;
    if (id < event_id || id > event_id + 2) return;
    rsf_unity_taken_packet taken{address};
    if (!taken.taken) return;
    const auto& packet = taken.packet;
    if (!active()) return;
    if (renderer == kUnityGfxRendererVulkan) {
        uint32_t change = 0;
        const bool reconfigure = id == event_id && rsf_unity_frames_pending_change(&change);
        if (rsf_uvk_render(id, packet, services, reconfigure) && reconfigure) rsf_unity_frames_settle_change(change);
    }
    else if (unity11) {
        rsf_unity_d3d11_render_packet(&services, static_cast<uint32_t>(id - event_id),
            rsf_unity_package_is_editor(), &packet);
    }
} catch (...) {
    rsf_unity_frame_callback call;
    if (call.entered) log("Unity DX11 bridge refused its render event.");
}
void cpu_fact(const rsf_game_cpu_event* fact) {
    rsf_unity_frame_callback call;
    if (call.entered && active() && fact && fact->struct_size>=sizeof(*fact) && fact->session_id==services.session_id && services.cpu_event)
        services.cpu_event(services.user,fact);
}
void cpu(uint32_t stage, uint64_t frame)
{
    rsf_unity_frame_callback call;
    if (!call.entered || !active() || !frame || !services.cpu_event) return;
    LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    rsf_unity_frames_cpu_stage(&services, stage, frame, uint64_t(now.QuadPart), uint64_t(frequency.QuadPart));
}
int configuration(uint32_t width, uint32_t height, rsf_unity_config* output)
{
    rsf_unity_frame_callback call;
    if (!call.entered) return 0;
    return rsf_unity_frames_config(&services, width, height, output);
}
void* enqueue(const rsf_unity_packet* packet)
{
    std::lock_guard<std::mutex> lock(guard);
    // The store retains D3D11 textures before the stored address reaches Unity's render event.
    if (renderer == kUnityGfxRendererD3D11) return rsf_unity_frames_admit_leased_packet(packet, services.session_id, &texture_ops);
    uint32_t slot = 0;
    return rsf_unity_frames_admit_packet(packet, services.session_id, &slot);
}
bool try_close_shared_bridge()
{
    std::lock_guard<std::mutex> lock(guard);
    if (!rsf_unity_frames_try_close()) return false;
    // Read completion only after admission closes. Callbacks own the fence while recording.
    if (renderer == kUnityGfxRendererVulkan) return true;
    return rsf_unity_d3d11_try_close();
}
}

extern "C" void* rsf_unity_d3d11_engine_device()
{
    return unity11 ? unity11->GetDevice() : nullptr;
}
extern "C" void* rsf_unity_d3d11_engine_swapchain()
{
    return unity11->GetSwapChain();
}
extern "C" bool rsf_unity_d3d11_runtime_graphics(void** device, void** queue)
{
    rsf_streamline_graphics graphics{};
    graphics.struct_size = sizeof(graphics);
    if (!rsf_d3d11_present_graphics(&graphics)) return false;
    auto* present = rsf_d3d11_present_host();
    *device = graphics.native_device;
    *queue = present ? rsf_streamline_host_native(present, graphics.queue) : graphics.queue;
    return true;
}
extern "C" void rsf_unity_d3d11_report_creation_failure(const char* message)
{
    rsf_unity_package_bridge_error(RSF_BACKEND_ERROR_INIT_FAILED, message);
}

bool rsf_unity_bridge_prepare(const rsf_game_host_services& host, rsf_unity_native_api* output) noexcept
{
    if (renderer == kUnityGfxRendererD3D12) return rsf_unity_d3d12_prepare(host, output);
    if (!output || !host.render_pass || !host.render_config || !host.log) return false;
    std::lock_guard<std::mutex> lock(guard);
    services = host;
    *output = {sizeof(*output), RSF_UNITY_NATIVE_ABI_VERSION, host.session_id,
        [](const char* text) { rsf_unity_frame_callback call; if (call.entered) log(text); }, configuration, enqueue,
        reinterpret_cast<void*>(render_event), rsf_unity_frames_report_managed, cpu, cpu_fact}; return true;
}
void rsf_unity_bridge_activate(bool enabled) noexcept
{
    if (renderer == kUnityGfxRendererD3D12) rsf_unity_d3d12_activate(enabled); else rsf_unity_frames_activate(enabled ? 1u : 0u);
}
bool rsf_unity_package_bridge_try_close() noexcept
{
    rsf_unity_bridge_activate(false);
    const bool vulkan = renderer == kUnityGfxRendererVulkan;
    if (vulkan) rsf_unity_vulkan_stop_admission();
    if (!(renderer == kUnityGfxRendererD3D12 ? rsf_unity_d3d12_try_close() : try_close_shared_bridge())) return false;
    if (vulkan && !rsf_unity_vulkan_try_close()) return false;
    if (!rsf_unity_package_is_editor() || rsf_unity_preview_drained()) return true;
    if (vulkan) rsf_unity_vulkan_finish_retirement(false);
    return false;
}
void rsf_unity_package_bridge_finish_retirement(bool commit) noexcept
{
    const bool vulkan = renderer == kUnityGfxRendererVulkan;
    if (commit && rsf_unity_package_is_editor()) rsf_unity_preview_release();
    if (vulkan) rsf_unity_vulkan_finish_retirement(commit);
    if (!commit) return;
    if (renderer == kUnityGfxRendererD3D12) {
        rsf_unity_d3d12_release();
        rsf_unity_package_sr_slots_released(12);
        return;
    }
    std::lock_guard<std::mutex> lock(guard);
    // The Vulkan bridge releases the ring itself, only together with the objects that used it.
    // Otherwise the D3D11 record destroys its surfaces, slots, fences, queue and devices, then
    // forgets the ring. It refuses without the checked close and keeps everything.
    if (!vulkan) {
        if (rsf_unity_d3d11_release()) {
            rsf_unity_package_sr_slots_released(11);
        } else {
            log("Unity DX11 bridge release refused without a checked close; its graphics remain owned.");
        }
    }
    services = {};
    // Releases any packet lease still held. The frame owner stays closed. The next session
    // activates it after preparation.
    rsf_unity_frames_release();
}
bool rsf_unity_package_bridge_open(bool editor)
{
    if (renderer == kUnityGfxRendererVulkan) {
        rsf_unity_vulkan_open();
        return editor || rsf_uvk_create_window();
    }
    if (renderer == kUnityGfxRendererD3D11) rsf_unity_d3d11_open();
    return renderer == kUnityGfxRendererD3D11 || renderer == kUnityGfxRendererD3D12;
}
uint32_t rsf_unity_bridge_managed_state() noexcept
{
    return renderer == kUnityGfxRendererD3D12 ? rsf_unity_d3d12_managed_state() : rsf_unity_frames_managed_stage();
}
extern "C" __declspec(dllexport) void* rsf_unity_get_render_event() noexcept
{
    return renderer == kUnityGfxRendererD3D12 ? rsf_unity_d3d12_get_render_event() :
        renderer == kUnityGfxRendererD3D11 || renderer == kUnityGfxRendererVulkan ? reinterpret_cast<void*>(render_event) : nullptr;
}
extern "C" __declspec(dllexport) int rsf_unity_get_event_id() noexcept { return renderer == kUnityGfxRendererD3D12 ? rsf_unity_d3d12_get_event_id() : event_id; }
extern "C" __declspec(dllexport) int rsf_unity_set_engine_spatial(uint32_t engine_spatial, uint32_t requested_quality) noexcept
{
    if (renderer == kUnityGfxRendererD3D12) return rsf_unity_d3d12_set_engine_spatial(engine_spatial, requested_quality);
    return rsf_unity_frames_set_policy(engine_spatial, requested_quality);
}
extern "C" void UNITY_INTERFACE_API rsf_unity_package_bridge_load(IUnityInterfaces* interfaces)
{
    auto* graphics = interfaces->Get<IUnityGraphics>(); if (!graphics) return;
    renderer = graphics->GetRenderer();
    if (renderer == kUnityGfxRendererD3D12) {
        rsf_unity_d3d12_load(interfaces); unity12 = interfaces->Get<IUnityGraphicsD3D12v7>();
        if (unity12) {
            UnityD3D12PluginEventConfig config{}; config.graphicsQueueAccess = kUnityD3D12GraphicsQueueAccess_Allow;
            config.flags = kUnityD3D12EventConfigFlag_SyncWorkerThreads | kUnityD3D12EventConfigFlag_FlushCommandBuffers;
            config.ensureActiveRenderTextureIsBound = true;
            unity12->ConfigureEvent(rsf_unity_d3d12_get_event_id() + 1, &config);
        }
    }
    else if (renderer == kUnityGfxRendererD3D11) { unity11 = interfaces->Get<IUnityGraphicsD3D11>(); event_id = graphics->ReserveEventIDRange(3); }
    else if (renderer == kUnityGfxRendererVulkan) { event_id = graphics->ReserveEventIDRange(3); rsf_uvk_load(interfaces, event_id); }
}
extern "C" void UNITY_INTERFACE_API rsf_unity_package_bridge_unload()
{
    if (renderer == kUnityGfxRendererD3D12) rsf_unity_d3d12_unload();
    if (renderer == kUnityGfxRendererVulkan) rsf_uvk_unload();
    rsf_unity_frames_activate(0); unity11 = nullptr; unity12 = nullptr;
    renderer = kUnityGfxRendererNull;
}
bool rsf_unity_package_graphics(Microsoft::WRL::ComPtr<ID3D12Device>& owning_device,
    Microsoft::WRL::ComPtr<ID3D12CommandQueue>& owning_queue) noexcept
{
    if (renderer == kUnityGfxRendererD3D12 && unity12) {
        owning_device = unity12->GetDevice(); owning_queue = unity12->GetCommandQueue();
        return owning_device && owning_queue;
    }
    if (renderer == kUnityGfxRendererD3D11) {
        // Take the caller's references while a frame callback keeps the record published.
        rsf_unity_frame_callback call;
        void* device = nullptr;
        void* queue = nullptr;
        if (!call.entered || !rsf_unity_d3d11_graphics(&device, &queue)) return false;
        owning_device.Attach(static_cast<ID3D12Device*>(device));
        owning_queue.Attach(static_cast<ID3D12CommandQueue*>(queue));
        return true;
    }
    return renderer == kUnityGfxRendererVulkan && rsf_uvk_graphics(owning_device, owning_queue);
}
