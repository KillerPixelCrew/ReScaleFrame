// SPDX-License-Identifier: GPL-3.0-only
#include "bridge.h"
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
#include <atomic>
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
IUnityGraphicsD3D11* unity11 = nullptr;
IUnityGraphicsD3D12v7* unity12 = nullptr;
UnityGfxRenderer renderer = kUnityGfxRendererNull;
rsf_game_host_services services{};
std::atomic<bool> active{false}, changing{false};
std::atomic<uint32_t> generation{1}, quality{1}, spatial{0}, managed{0}, callbacks{0};
int event_id = 0;
std::mutex packets_guard;
struct Pending { rsf_unity_packet packet{}; std::array<ComPtr<IUnknown>, 4> leases; bool used = false; };
std::array<Pending, 32> packets;
struct Surface {
    rsf_shared_surface* surface = nullptr;
    D3D11_TEXTURE2D_DESC description{};
    ~Surface() { rsf_shared_surface_destroy(surface); }
};
struct Commands { ComPtr<ID3D12CommandAllocator> allocator; ComPtr<ID3D12GraphicsCommandList> list; uint64_t complete = 0; };
ComPtr<ID3D11Device5> device11;
ComPtr<ID3D11DeviceContext4> context11;
ComPtr<ID3D12Device> device12;
ComPtr<ID3D12CommandQueue> queue12;
ComPtr<ID3D12Fence> completion;
std::array<Commands, 3> commands;
std::array<Surface, 6> surfaces;
rsf_shared_fence* sharing_fence = nullptr;
uint64_t submitted = 0, shared_value = 0;
uint32_t next_command = 0;
bool initialized11 = false, failed11 = false;
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
    ComPtr<ID3D11DeviceContext> context; device->GetImmediateContext(&context);
    if (FAILED(device.As(&device11)) || FAILED(context.As(&context11))) return false;
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
    if (!wait(submitted)) return false;
    DXGI_FORMAT format = index == 1 ? DXGI_FORMAT_R32_FLOAT : index == 2 ? DXGI_FORMAT_R16G16_FLOAT : desc.Format;
    if (format == DXGI_FORMAT_R16G16B16A16_TYPELESS) format = DXGI_FORMAT_R16G16B16A16_FLOAT;
    if (format == DXGI_FORMAT_R8G8B8A8_TYPELESS || format == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB) format = DXGI_FORMAT_R8G8B8A8_UNORM;
    if (format == DXGI_FORMAT_B8G8R8A8_TYPELESS || format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB) format = DXGI_FORMAT_B8G8R8A8_UNORM;
    rsf_shared_surface_setup setup{sizeof(setup), RSF_SHARED_SURFACE_ABI_VERSION, desc.Width, desc.Height,
        static_cast<uint32_t>(format), 1, services.log, services.user};
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
void execute11(int id, const rsf_unity_packet& packet)
{
    if (!active.load() || !initialize11() || !services.render_pass) return;
    rsf_game_render_pass pass{}; make_pass(packet, pass);
    if (id == event_id + 1) {
        pass.role = RSF_GAME_RENDER_WINDOW; pass.swapchain = unity11->GetSwapChain();
        if (rsf_unity_package_is_editor() && packet.color) {
            ComPtr<ID3D11Texture2D> source;
            if (FAILED(static_cast<IUnknown*>(packet.color)->QueryInterface(IID_PPV_ARGS(&source))) || !prepare_surface(5, source.Get())) return;
            context11->CopyResource(static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(surfaces[5].surface)), source.Get());
            auto* fence11 = static_cast<ID3D11Fence*>(rsf_shared_fence_d3d11(sharing_fence));
            auto* fence12 = static_cast<ID3D12Fence*>(rsf_shared_fence_d3d12(sharing_fence));
            const auto uploaded = ++shared_value, consumed = ++shared_value;
            if (FAILED(context11->Signal(fence11, uploaded))) return;
            context11->Flush(); if (FAILED(queue12->Wait(fence12, uploaded))) return;
            pass.color_input = rsf_shared_surface_d3d12(surfaces[5].surface); pass.flags |= RSF_UNITY_PREVIEW_SOURCE_COMMON;
            services.render_pass(services.user, queue12.Get(), &pass, 1);
            if (FAILED(queue12->Signal(fence12, consumed)) || FAILED(context11->Wait(fence11, consumed))) return;
            ++submitted; (void)queue12->Signal(completion.Get(), submitted); return;
        }
        services.render_pass(services.user, queue12.Get(), &pass, 1); return;
    }
    auto& slot = commands[next_command];
    if (!wait(slot.complete)) { log("Unity DX11 command-slot retirement timed out."); return; }
    if (!slot.list) {
        if (FAILED(device12->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&slot.allocator))) ||
            FAILED(device12->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, slot.allocator.Get(), nullptr, IID_PPV_ARGS(&slot.list))) || FAILED(slot.list->Close())) return;
    }
    if (FAILED(slot.allocator->Reset()) || FAILED(slot.list->Reset(slot.allocator.Get(), nullptr))) return;
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
    const uint64_t upload = ++shared_value, download = ++shared_value;
    if (FAILED(context11->Signal(fence11, upload))) return;
    context11->Flush(); if (FAILED(queue12->Wait(fence12, upload))) return;
    std::array<ID3D12Resource*, 4> resources{};
    for (uint32_t i = 0; i < 4; ++i) if (input[i]) {
        resources[i] = static_cast<ID3D12Resource*>(rsf_shared_surface_d3d12(surfaces[hudless ? 4u : i].surface));
        barrier(slot.list.Get(), resources[i], D3D12_RESOURCE_STATE_COMMON,
            hudless ? D3D12_RESOURCE_STATE_RENDER_TARGET : i == 3 ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    }
    pass.color_input = resources[0]; pass.depth = resources[1]; pass.motion = resources[2]; pass.color_output = resources[3];
    if (hudless) pass.role = RSF_GAME_RENDER_FINAL_SCENE;
    if (!hudless && changing.exchange(false)) {
        if (!wait(submitted)) { changing.store(true); return; }
        pass.flags |= RSF_GAME_RENDER_RECONFIGURE; pass.camera_valid = 0;
    }
    services.render_pass(services.user, slot.list.Get(), &pass, 1);
    for (uint32_t i = 0; i < 4; ++i) if (resources[i]) barrier(slot.list.Get(), resources[i],
        hudless ? D3D12_RESOURCE_STATE_RENDER_TARGET : i == 3 ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
    if (FAILED(slot.list->Close())) return;
    ID3D12CommandList* lists[] = {slot.list.Get()}; queue12->ExecuteCommandLists(1, lists);
    if (FAILED(queue12->Signal(fence12, download)) || FAILED(context11->Wait(fence11, download))) return;
    if (input[3] && !hudless) context11->CopyResource(input[3].Get(), static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(surfaces[3].surface)));
    slot.complete = ++submitted; if (FAILED(queue12->Signal(completion.Get(), submitted))) { active.store(false); return; }
    services.render_pass(services.user, slot.list.Get(), &pass, 0);
    next_command = (next_command + 1) % 3;
}
void UNITY_INTERFACE_API render_event(int id, void* address) noexcept try
{
    if (id < event_id || id > event_id + 2) return;
    callbacks.fetch_add(1);
    struct End { ~End() { callbacks.fetch_sub(1); } } end;
    Pending packet;
    { std::lock_guard<std::mutex> lock(packets_guard);
      for (auto& entry : packets) if (&entry.packet == address && entry.used) { packet = std::move(entry); entry = {}; break; } }
    if (packet.used) {
        if (renderer == kUnityGfxRendererVulkan) {
            const bool reconfigure = id == event_id && changing.load();
            if (rsf_uvk_render(id, packet.packet, services, reconfigure) && reconfigure) changing.store(false);
        }
        else execute11(id, packet.packet);
    }
} catch (...) { log("Unity DX11 bridge refused its render event."); }
void state(uint32_t value) { managed.store(value); }
void cpu(uint32_t stage, uint64_t frame)
{
    if (!active.load() || !frame || !services.cpu_event) return;
    LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    rsf_game_cpu_event event{sizeof(event), stage, services.session_id, frame, static_cast<uint64_t>(now.QuadPart), static_cast<uint64_t>(frequency.QuadPart), 0, 0, 0};
    services.cpu_event(services.user, &event);
}
int configuration(uint32_t width, uint32_t height, rsf_unity_config* output)
{
    if (!output || output->struct_size != sizeof(*output) || output->abi_version != RSF_UNITY_BRIDGE_ABI_VERSION) return 0;
    rsf_game_render_config plan{}; plan.struct_size = sizeof(plan);
    if (!services.render_config || !services.render_config(services.user, &plan)) return 0;
    *output = {sizeof(*output), RSF_UNITY_BRIDGE_ABI_VERSION,
        active.load() && !changing.load() && plan.output_width == width && plan.output_height == height ? plan.enabled : 0u,
        spatial.load(), quality.load(), generation.load(), plan.render_width, plan.render_height, width, height}; return 1;
}
void* enqueue(const rsf_unity_packet* packet)
{
    if (!packet || !active.load() || packet->struct_size != sizeof(*packet) || packet->abi_version != RSF_UNITY_BRIDGE_ABI_VERSION ||
        packet->session_id != services.session_id || !packet->frame_id || !packet->view_key ||
        !packet->camera.render_width || !packet->camera.render_height || !packet->camera.output_width || !packet->camera.output_height) return nullptr;
    std::lock_guard<std::mutex> lock(packets_guard);
    for (auto& entry : packets) if (!entry.used) {
        entry.packet = *packet; void* pointers[] = {packet->color, packet->depth, packet->motion, packet->output};
        if (renderer == kUnityGfxRendererD3D11)
            for (size_t i = 0; i < 4; ++i) entry.leases[i] = static_cast<IUnknown*>(pointers[i]);
        entry.used = true; return &entry.packet;
    }
    return nullptr;
}
}

bool rsf_unity_bridge_prepare(const rsf_game_host_services& host, rsf_unity_native_api* output) noexcept
{
    if (renderer == kUnityGfxRendererD3D12) return rsf_unity_d3d12_prepare(host, output);
    if (!output || !host.render_pass || !host.render_config || !host.log) return false;
    services = host;
    *output = {sizeof(*output), RSF_UNITY_NATIVE_ABI_VERSION, host.session_id, [](const char* text) { log(text); }, configuration, enqueue,
        reinterpret_cast<void*>(render_event), state, cpu}; return true;
}
void rsf_unity_bridge_activate(bool enabled) noexcept
{
    if (renderer == kUnityGfxRendererD3D12) rsf_unity_d3d12_activate(enabled); else active.store(enabled);
}
bool rsf_unity_bridge_drained() noexcept
{
    if (renderer == kUnityGfxRendererD3D12) return rsf_unity_d3d12_drained();
    if (callbacks.load()) return false;
    std::lock_guard<std::mutex> lock(packets_guard);
    for (const auto& entry : packets) if (entry.used) return false;
    return renderer == kUnityGfxRendererVulkan ? rsf_uvk_drained() : !submitted || (completion && completion->GetCompletedValue() >= submitted);
}
void rsf_unity_bridge_release() noexcept
{
    if (renderer == kUnityGfxRendererD3D12) { rsf_unity_d3d12_release(); return; }
    if (!rsf_unity_bridge_drained()) return;
    if (renderer == kUnityGfxRendererVulkan) rsf_uvk_release();
    for (auto& surface : surfaces) { rsf_shared_surface_destroy(surface.surface); surface.surface = nullptr; }
    rsf_shared_fence_destroy(sharing_fence); sharing_fence = nullptr;
    commands = {}; completion.Reset(); queue12.Reset(); device12.Reset(); context11.Reset(); device11.Reset();
    submitted = shared_value = next_command = 0; services = {}; managed.store(0);
    initialized11 = failed11 = false;
}
uint32_t rsf_unity_bridge_managed_state() noexcept { return renderer == kUnityGfxRendererD3D12 ? rsf_unity_d3d12_managed_state() : managed.load(); }
extern "C" __declspec(dllexport) void* rsf_unity_get_render_event() noexcept
{
    return renderer == kUnityGfxRendererD3D12 ? rsf_unity_d3d12_get_render_event() :
        renderer == kUnityGfxRendererD3D11 || renderer == kUnityGfxRendererVulkan ? reinterpret_cast<void*>(render_event) : nullptr;
}
extern "C" __declspec(dllexport) int rsf_unity_get_event_id() noexcept { return renderer == kUnityGfxRendererD3D12 ? rsf_unity_d3d12_get_event_id() : event_id; }
extern "C" __declspec(dllexport) int rsf_unity_set_engine_spatial(uint32_t engine_spatial, uint32_t requested_quality) noexcept
{
    if (renderer == kUnityGfxRendererD3D12) return rsf_unity_d3d12_set_engine_spatial(engine_spatial, requested_quality);
    spatial.store(engine_spatial); quality.store(requested_quality); generation.fetch_add(1); if (active.load()) changing.store(true); return 1;
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
    active.store(false); unity11 = nullptr; unity12 = nullptr;
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
