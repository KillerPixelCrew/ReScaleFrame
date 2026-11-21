// SPDX-License-Identifier: MIT
// The Unity package's D3D11 and Vulkan render bridge; D3D12 routes to the shared D3D12 bridge.
// Packet admission and storage, the engine policy and its generation, activity, the managed stage
// and callback retirement are the shared bridge state (bridge_state.rs). The D3D11 path's command
// ring, completion values and shared fence values are bridge_interop.rs, which the Vulkan bridge
// (vulkan_bridge.cpp, its own state in vulkan_state.rs) uses for its slots too. This file keeps
// the Unity interfaces, D3D11/D3D12/DXGI objects and calls, shared surfaces and fences, the D3D11
// texture leases, render-pass and CPU-event calls and the exported entry points.
#include "bridge.h"
#include "bridge_state.h"
#include "bridge_interop.h"
#include "vulkan_bridge.h"
#include "package.h"
#include <rescaleframe/shared_surface.h>
#include <windows.h>
#include <d3d11_4.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <IUnityGraphics.h>
#include <IUnityGraphicsD3D11.h>
#include <IUnityGraphicsD3D12.h>
#include <rescaleframe/d3d11_present_bridge.h>
#include <array>
#include <mutex>
#include <cstring>
#include <cmath>

extern bool rsf_unity_d3d12_prepare(const rsf_game_host_services&, rsf_unity_native_api*) noexcept;
extern void rsf_unity_d3d12_activate(bool) noexcept;
extern bool rsf_unity_d3d12_drained() noexcept;
extern void rsf_unity_d3d12_release() noexcept;
extern uint32_t rsf_unity_d3d12_managed_state() noexcept;
extern "C" void UNITY_INTERFACE_API rsf_unity_d3d12_load(IUnityInterfaces*);
extern "C" void UNITY_INTERFACE_API rsf_unity_d3d12_unload();
extern "C" void* rsf_unity_d3d12_get_render_event() noexcept;
extern "C" int rsf_unity_d3d12_get_event_id() noexcept;
extern "C" int rsf_unity_d3d12_set_engine_spatial(uint32_t, uint32_t) noexcept;

namespace {
using Microsoft::WRL::ComPtr;
// D3D11 texture references a stored packet holds from enqueue until its render event ends.
// Vulkan packets carry image handles, which are not reference counted.
using Leases = std::array<ComPtr<IUnknown>, 4>;
IUnityGraphicsD3D11* unity11 = nullptr;
IUnityGraphicsD3D12v7* unity12 = nullptr;
UnityGfxRenderer renderer = kUnityGfxRendererNull;
// Guards the host services and the packet leases, and orders them with the Rust packet store.
std::mutex guard;
rsf_game_host_services services{};
std::array<Leases, RSF_UNITY_FRAMES_PACKETS> pending_leases;
int event_id = 0;
struct Surface {
    rsf_shared_surface* surface = nullptr;
    D3D11_TEXTURE2D_DESC description{};
    ~Surface() { rsf_shared_surface_destroy(surface); }
};
struct Commands { ComPtr<ID3D12CommandAllocator> allocator; ComPtr<ID3D12GraphicsCommandList> list; };
// Closes a reset list on every early return, so the slot's allocator resets on its next use.
struct Recording {
    ID3D12GraphicsCommandList* list = nullptr;
    ~Recording() { if (list) list->Close(); }
    bool close() { auto* open = list; list = nullptr; return SUCCEEDED(open->Close()); }
};
ComPtr<ID3D11Device5> device11;
ComPtr<ID3D11DeviceContext4> context11;
ComPtr<ID3D12Device> device12;
ComPtr<ID3D12CommandQueue> queue12;
ComPtr<ID3D12Fence> completion;
std::array<Commands, RSF_UNITY_FRAMES_SLOTS> commands;
std::array<Surface, 6> surfaces;
rsf_shared_fence* sharing_fence = nullptr;
bool initialized11 = false, failed11 = false;
bool active() { return rsf_unity_frames_active() != 0; }
// The bridge's completion fence value, or 0 without a fence.
uint64_t completed() { return completion ? completion->GetCompletedValue() : 0; }
void log(const char* text) { if (services.log) services.log(services.user, text); }
bool wait(uint64_t value)
{
    if (!value || (completion && completion->GetCompletedValue() >= value)) return true;
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!event) return false;
    const bool result = completion && SUCCEEDED(completion->SetEventOnCompletion(value, event)) && WaitForSingleObject(event, 10000) == WAIT_OBJECT_0;
    CloseHandle(event); return result;
}
void barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* resource, D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after)
{
    if (!resource || before == after) return;
    D3D12_RESOURCE_BARRIER change{}; change.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    change.Transition = {resource, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, before, after}; list->ResourceBarrier(1, &change);
}
bool create11()
{
    if (!unity11) return false;
    ComPtr<ID3D11Device> device = unity11->GetDevice();
    if (!device) return false;
    ComPtr<ID3D11DeviceContext> context; device->GetImmediateContext(&context);
    if (!context || FAILED(device.As(&device11)) || FAILED(context.As(&context11))) return false;
    rsf_streamline_graphics graphics{}; graphics.struct_size = sizeof(graphics);
    if (rsf_d3d11_present_graphics(&graphics)) {
        // Shared resources report their native owner. Record/execute native lists too,
        // so SR's device identity checks compare the same COM interface throughout.
        device12 = static_cast<ID3D12Device*>(graphics.native_device);
        auto* host = rsf_d3d11_present_host();
        void* native_queue = host ? rsf_streamline_host_native(host, graphics.queue) : graphics.queue;
        if (!device12 || !native_queue) return false;
        queue12 = static_cast<ID3D12CommandQueue*>(native_queue);
    } else {
        ComPtr<IDXGIDevice> dxgi; ComPtr<IDXGIAdapter> adapter;
        if (FAILED(device.As(&dxgi)) || FAILED(dxgi->GetAdapter(&adapter)) ||
            FAILED(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device12)))) return false;
        D3D12_COMMAND_QUEUE_DESC desc{};
        if (FAILED(device12->CreateCommandQueue(&desc, IID_PPV_ARGS(&queue12)))) return false;
    }
    if (rsf_shared_fence_create(device11.Get(), device12.Get(), services.log, services.user, &sharing_fence) != RSF_SHARED_OK ||
        FAILED(device12->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&completion)))) return false;
    return true;
}
bool initialize11()
{
    if (initialized11 || failed11) return initialized11;
    initialized11 = create11(); failed11 = !initialized11;
    if (failed11) rsf_unity_package_bridge_error(RSF_BACKEND_ERROR_INIT_FAILED, "Unity DX11 shared device/fence initialization failed; native output remains in place.");
    return initialized11;
}
bool prepare_surface(uint32_t index, ID3D11Texture2D* texture)
{
    if (!texture) return false;
    D3D11_TEXTURE2D_DESC desc{}; texture->GetDesc(&desc);
    auto& surface = surfaces[index];
    if (surface.surface && desc.Width == surface.description.Width && desc.Height == surface.description.Height && desc.Format == surface.description.Format) return true;
    if (!wait(rsf_unity_interop_submitted())) return false;
    DXGI_FORMAT format = index == 1 ? DXGI_FORMAT_R32_FLOAT : index == 2 ? DXGI_FORMAT_R16G16_FLOAT : desc.Format;
    if (format == DXGI_FORMAT_R16G16B16A16_TYPELESS) format = DXGI_FORMAT_R16G16B16A16_FLOAT;
    if (format == DXGI_FORMAT_R8G8B8A8_TYPELESS || format == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB) format = DXGI_FORMAT_R8G8B8A8_UNORM;
    if (format == DXGI_FORMAT_B8G8R8A8_TYPELESS || format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB) format = DXGI_FORMAT_B8G8R8A8_UNORM;
    rsf_shared_surface_setup setup{sizeof(setup), RSF_SHARED_SURFACE_ABI_VERSION, desc.Width, desc.Height,
        static_cast<uint32_t>(format), 1, services.log, services.user, 0};
    rsf_shared_surface* replacement = nullptr;
    if (rsf_shared_surface_create(device11.Get(), device12.Get(), &setup, &replacement) != RSF_SHARED_OK) return false;
    rsf_shared_surface_destroy(surface.surface); surface.surface = replacement; surface.description = desc;
    return true;
}
void make_pass(const rsf_unity_packet& packet, rsf_game_render_pass& pass)
{
    pass = {}; pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_SR;
    pass.session_id = packet.session_id; pass.family_key = pass.view_key = pass.history_key = packet.view_key;
    pass.native_frame = pass.source_frame_id = packet.frame_id; pass.resource_generation = packet.generation;
    pass.screen = RSF_SCREEN_FLIGHT; pass.flags = packet.flags & RSF_UNITY_PACKET_RESET;
    pass.camera = packet.camera; pass.camera_valid = (packet.flags & RSF_UNITY_PACKET_PROBE) ? 0u : 1u;
    pass.render_rect[2] = static_cast<int32_t>(packet.camera.render_width); pass.render_rect[3] = static_cast<int32_t>(packet.camera.render_height);
    pass.output_rect[2] = static_cast<int32_t>(packet.camera.output_width); pass.output_rect[3] = static_cast<int32_t>(packet.camera.output_height);
    pass.motion_to_uv[0] = pass.motion_to_uv[1] = 1; pass.motion_camera_included = 1;
    std::memcpy(pass.previous_clip_to_clip, packet.previous_clip_to_clip, sizeof(pass.previous_clip_to_clip));
}
bool retire_interop_submission(uint64_t value, bool handed_back)
{
    // Queue work has already gone out. Publish the retirement fence even if the D3D11
    // wait or shared-fence signal refused, so release still waits for the submitted work.
    const bool signalled = value && SUCCEEDED(queue12->Signal(completion.Get(), value));
    if (!signalled || !handed_back) {
        rsf_unity_frames_activate(0);
        log(signalled ? "Unity DX11 shared-surface hand-back failed; adapter stopped."
                     : "Unity DX11 completion signal failed; adapter stopped.");
        return false;
    }
    return true;
}
void execute11(int id, const rsf_unity_packet& packet)
{
    if (!active() || !unity11 || !initialize11() || !services.render_pass) return;
    rsf_game_render_pass pass{}; make_pass(packet, pass);
    if (id == event_id + 1) {
        pass.role = RSF_GAME_RENDER_WINDOW; pass.swapchain = unity11->GetSwapChain();
        if (rsf_unity_package_is_editor() && packet.color) {
            ComPtr<ID3D11Texture2D> source;
            if (FAILED(static_cast<IUnknown*>(packet.color)->QueryInterface(IID_PPV_ARGS(&source))) || !prepare_surface(5, source.Get())) return;
            context11->CopyResource(static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(surfaces[5].surface)), source.Get());
            auto* fence11 = static_cast<ID3D11Fence*>(rsf_shared_fence_d3d11(sharing_fence));
            auto* fence12 = static_cast<ID3D12Fence*>(rsf_shared_fence_d3d12(sharing_fence));
            uint64_t uploaded = 0, consumed = 0;
            if (!rsf_unity_interop_shared_pair(&uploaded, &consumed) || FAILED(context11->Signal(fence11, uploaded))) return;
            context11->Flush(); if (FAILED(queue12->Wait(fence12, uploaded))) return;
            pass.color_input = rsf_shared_surface_d3d12(surfaces[5].surface); pass.flags |= RSF_UNITY_PREVIEW_SOURCE_COMMON;
            services.render_pass(services.user, queue12.Get(), &pass, 1);
            const bool handed_back = SUCCEEDED(queue12->Signal(fence12, consumed)) &&
                SUCCEEDED(context11->Wait(fence11, consumed));
            (void)retire_interop_submission(rsf_unity_interop_completion(), handed_back);
            return;
        }
        services.render_pass(services.user, queue12.Get(), &pass, 1); return;
    }
    uint64_t retired = 0;
    const uint32_t index = rsf_unity_interop_slot(&retired);
    if (index >= commands.size()) return;
    auto& slot = commands[index];
    if (!wait(retired)) { log("Unity DX11 command-slot retirement timed out."); return; }
    if (!slot.list) {
        if (FAILED(device12->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&slot.allocator))) ||
            FAILED(device12->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, slot.allocator.Get(), nullptr, IID_PPV_ARGS(&slot.list))) || FAILED(slot.list->Close())) return;
    }
    if (FAILED(slot.allocator->Reset()) || FAILED(slot.list->Reset(slot.allocator.Get(), nullptr))) return;
    Recording recording{slot.list.Get()};
    std::array<ComPtr<ID3D11Texture2D>, 4> input;
    void* pointers[] = {packet.color, packet.depth, packet.motion, packet.output};
    const bool hudless = id == event_id + 2;
    if (hudless && !pointers[0]) {
        auto* chain = unity11->GetSwapChain();
        if (!chain || FAILED(chain->GetBuffer(0, IID_PPV_ARGS(&input[0])))) return;
    }
    for (uint32_t i = 0; i < 4; ++i) {
        if (pointers[i] && FAILED(static_cast<IUnknown*>(pointers[i])->QueryInterface(IID_PPV_ARGS(&input[i])))) return;
        if (!input[i]) continue;
        const uint32_t surface = hudless ? 4u : i;
        if (!prepare_surface(surface, input[i].Get())) return;
        context11->CopyResource(static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(surfaces[surface].surface)), input[i].Get());
    }
    auto* fence11 = static_cast<ID3D11Fence*>(rsf_shared_fence_d3d11(sharing_fence));
    auto* fence12 = static_cast<ID3D12Fence*>(rsf_shared_fence_d3d12(sharing_fence));
    uint64_t upload = 0, download = 0;
    if (!rsf_unity_interop_shared_pair(&upload, &download) || FAILED(context11->Signal(fence11, upload))) return;
    context11->Flush(); if (FAILED(queue12->Wait(fence12, upload))) return;
    std::array<ID3D12Resource*, 4> resources{};
    for (uint32_t i = 0; i < 4; ++i) if (input[i]) {
        resources[i] = static_cast<ID3D12Resource*>(rsf_shared_surface_d3d12(surfaces[hudless ? 4u : i].surface));
        barrier(slot.list.Get(), resources[i], D3D12_RESOURCE_STATE_COMMON,
            hudless ? D3D12_RESOURCE_STATE_RENDER_TARGET : i == 3 ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    }
    pass.color_input = resources[0]; pass.depth = resources[1]; pass.motion = resources[2]; pass.color_output = resources[3];
    if (hudless) pass.role = RSF_GAME_RENDER_FINAL_SCENE;
    // A policy change applies on the next reconstruction once every submission completed; it
    // stays pending, and config keeps the bridge disabled, until then.
    uint32_t change = 0;
    if (!hudless && rsf_unity_frames_pending_change(&change)) {
        if (!wait(rsf_unity_interop_submitted())) return;
        rsf_unity_frames_settle_change(change);
        pass.flags |= RSF_GAME_RENDER_RECONFIGURE; pass.camera_valid = 0;
    }
    services.render_pass(services.user, slot.list.Get(), &pass, 1);
    for (uint32_t i = 0; i < 4; ++i) if (resources[i]) barrier(slot.list.Get(), resources[i],
        hudless ? D3D12_RESOURCE_STATE_RENDER_TARGET : i == 3 ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
    if (!recording.close()) return;
    ID3D12CommandList* lists[] = {slot.list.Get()}; queue12->ExecuteCommandLists(1, lists);
    const bool handed_back = SUCCEEDED(queue12->Signal(fence12, download)) &&
        SUCCEEDED(context11->Wait(fence11, download));
    if (handed_back && input[3] && !hudless)
        context11->CopyResource(input[3].Get(), static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(surfaces[3].surface)));
    if (!retire_interop_submission(rsf_unity_interop_submit(index), handed_back)) return;
    services.render_pass(services.user, slot.list.Get(), &pass, 0);
    rsf_unity_interop_advance(index);
}
void UNITY_INTERFACE_API render_event(int id, void* address) noexcept try
{
    if (id < event_id || id > event_id + 2) return;
    rsf_unity_frames_callback_enter();
    struct Finish { ~Finish() { rsf_unity_frames_callback_leave(); } } finish;
    rsf_unity_packet packet{};
    Leases leases;
    {
        std::lock_guard<std::mutex> lock(guard);
        uint32_t slot = 0;
        if (!rsf_unity_frames_take_packet(address, &packet, &slot) || slot >= pending_leases.size()) return;
        leases = std::move(pending_leases[slot]);
        pending_leases[slot] = {};
    }
    if (renderer == kUnityGfxRendererVulkan) {
        uint32_t change = 0;
        const bool reconfigure = id == event_id && rsf_unity_frames_pending_change(&change);
        if (rsf_uvk_render(id, packet, services, reconfigure) && reconfigure) rsf_unity_frames_settle_change(change);
    }
    else execute11(id, packet);
} catch (...) { log("Unity DX11 bridge refused its render event."); }
void cpu_fact(const rsf_game_cpu_event* fact) {
    if (active() && fact && fact->struct_size>=sizeof(*fact) && fact->session_id==services.session_id && services.cpu_event)
        services.cpu_event(services.user,fact);
}
void cpu(uint32_t stage, uint64_t frame)
{
    if (!active() || !frame || !services.cpu_event) return;
    LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    rsf_game_cpu_event event{sizeof(event), stage, services.session_id, frame, static_cast<uint64_t>(now.QuadPart), static_cast<uint64_t>(frequency.QuadPart), 0, 0, 0};
    event.event_version=RSF_GAME_CPU_EVENT_VERSION; event.flags=RSF_GAME_CPU_INPUT_KEYED;
    services.cpu_event(services.user, &event);
}
int configuration(uint32_t width, uint32_t height, rsf_unity_config* output)
{
    return rsf_unity_frames_config(&services, width, height, output);
}
void* enqueue(const rsf_unity_packet* packet)
{
    std::lock_guard<std::mutex> lock(guard);
    uint32_t slot = 0;
    void* stored = rsf_unity_frames_admit_packet(packet, services.session_id, &slot);
    if (!stored) return nullptr;
    // Lease D3D11 textures before the stored address reaches Unity's render event.
    if (renderer == kUnityGfxRendererD3D11) {
        void* textures[] = {packet->color, packet->depth, packet->motion, packet->output};
        auto& leases = pending_leases[slot];
        for (size_t i = 0; i < leases.size(); ++i) leases[i] = static_cast<IUnknown*>(textures[i]);
    }
    return stored;
}
// The shared D3D12 ring and sources stay unused on this path, so a completion of 0 checks only
// the render callbacks and stored packets there.
bool drained_locked()
{
    if (!rsf_unity_frames_drained(0)) return false;
    return renderer == kUnityGfxRendererVulkan ? rsf_uvk_drained() : rsf_unity_interop_drained(completed()) != 0;
}
}

bool rsf_unity_bridge_prepare(const rsf_game_host_services& host, rsf_unity_native_api* output) noexcept
{
    if (renderer == kUnityGfxRendererD3D12) return rsf_unity_d3d12_prepare(host, output);
    if (!output || !host.render_pass || !host.render_config || !host.log) return false;
    std::lock_guard<std::mutex> lock(guard);
    services = host;
    *output = {sizeof(*output), RSF_UNITY_NATIVE_ABI_VERSION, host.session_id, [](const char* text) { log(text); }, configuration, enqueue,
        reinterpret_cast<void*>(render_event), rsf_unity_frames_report_managed, cpu, cpu_fact}; return true;
}
void rsf_unity_bridge_activate(bool enabled) noexcept
{
    if (renderer == kUnityGfxRendererD3D12) rsf_unity_d3d12_activate(enabled); else rsf_unity_frames_activate(enabled ? 1u : 0u);
}
bool rsf_unity_bridge_drained() noexcept
{
    if (renderer == kUnityGfxRendererD3D12) return rsf_unity_d3d12_drained();
    std::lock_guard<std::mutex> lock(guard);
    return drained_locked();
}
void rsf_unity_bridge_release() noexcept
{
    if (renderer == kUnityGfxRendererD3D12) { rsf_unity_d3d12_release(); return; }
    std::lock_guard<std::mutex> lock(guard);
    if (!drained_locked()) return;
    if (renderer == kUnityGfxRendererVulkan) rsf_uvk_release();
    for (auto& surface : surfaces) { rsf_shared_surface_destroy(surface.surface); surface.surface = nullptr; }
    rsf_shared_fence_destroy(sharing_fence); sharing_fence = nullptr;
    commands = {}; completion.Reset(); queue12.Reset(); device12.Reset(); context11.Reset(); device11.Reset();
    pending_leases = {}; services = {};
    // Forgets the ring, the stored packets and the managed stage; activity and policy persist.
    // The Vulkan bridge releases the ring itself, only together with the objects that used it.
    if (renderer != kUnityGfxRendererVulkan) rsf_unity_interop_release();
    rsf_unity_frames_release();
    initialized11 = failed11 = false;
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
    rsf_unity_frames_activate(0); unity11 = nullptr; unity12 = nullptr;
}
bool rsf_unity_package_graphics(void** owning_device, void** owning_queue) noexcept
{
    if (!owning_device || !owning_queue) return false;
    if (renderer == kUnityGfxRendererD3D12 && unity12) {
        *owning_device = unity12->GetDevice(); *owning_queue = unity12->GetCommandQueue(); return *owning_device && *owning_queue;
    }
    if (renderer == kUnityGfxRendererD3D11 && initialized11) {
        *owning_device = device12.Get(); *owning_queue = queue12.Get(); return true;
    }
    return renderer == kUnityGfxRendererVulkan && rsf_uvk_graphics(owning_device, owning_queue);
}
