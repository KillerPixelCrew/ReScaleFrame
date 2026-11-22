// SPDX-License-Identifier: MIT
// The Unity package's D3D11 and Vulkan render bridge; D3D12 routes to the shared D3D12 bridge.
// Packet admission and storage, the engine policy and its generation, activity, the managed stage
// and callback retirement are the shared bridge state (bridge_state.rs), which also owns the D3D11
// texture references of queued packets from admission until their render event ends. The D3D11
// path's command ring, completion values and shared fence values are bridge_interop.rs, which the
// Vulkan bridge (vulkan_bridge.cpp, its own state in vulkan_state.rs) uses for its slots too. The
// D3D11 path's devices, immediate context, queue, completion and shared fences, command slots and
// six shared surfaces are one Rust record with its creation, surface keys, replacement and lease,
// checked close and release (d3d11_state.rs). Each D3D11 render event's route, assembled pass,
// surface slots and D3D12 states, the shared surface format, the policy change gate and the
// submission's retirement verdict are Rust too (d3d11_pass.rs). This file keeps the Unity
// interfaces, the typed D3D11/D3D12/DXGI calls as leaves (texture AddRef and Release among them)
// in their order, render-pass and CPU-event calls and the exported entry points.
#include "bridge.h"
#include "bridge_interop.h"
#include "d3d11_pass.h"
#include "d3d11_state.h"
#include "vulkan_bridge.h"
#include "vulkan_state.h"
#include "package.h"
#include "preview.h"
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
#include <cstddef>
#include <mutex>
#include <cstring>
#include <cmath>

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

static_assert(sizeof(rsf_unity_d3d11_device_handles) == 48, "d3d11_state.rs DeviceHandles");
static_assert(offsetof(rsf_unity_d3d11_device_handles, device12) == 16);
static_assert(offsetof(rsf_unity_d3d11_device_handles, sharing) == 40);
static_assert(sizeof(rsf_unity_d3d11_command_handles) == 16, "d3d11_state.rs CommandSlot");
static_assert(offsetof(rsf_unity_d3d11_command_handles, list) == 8);
static_assert(sizeof(rsf_unity_d3d11_device_ops) == 9 * sizeof(void*), "d3d11_state.rs DeviceOps");
static_assert(offsetof(rsf_unity_d3d11_pass_plan, surfaces) == 16, "d3d11_pass.rs PassPlan");
static_assert(offsetof(rsf_unity_d3d11_pass_plan, states) == 32);
static_assert(offsetof(rsf_unity_d3d11_pass_plan, pass) == 48);
static_assert(sizeof(rsf_unity_d3d11_pass_plan) == 48 + sizeof(rsf_game_render_pass));
// The D3D12 states d3d11_pass.rs plans and the DXGI formats it chooses.
static_assert(D3D12_RESOURCE_STATE_COMMON == 0 && D3D12_RESOURCE_STATE_RENDER_TARGET == 0x4 &&
    D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE == 0x40 && D3D12_RESOURCE_STATE_COPY_DEST == 0x400);
static_assert(DXGI_FORMAT_R16G16B16A16_TYPELESS == 9 && DXGI_FORMAT_R16G16B16A16_FLOAT == 10 &&
    DXGI_FORMAT_R8G8B8A8_TYPELESS == 27 && DXGI_FORMAT_R8G8B8A8_UNORM == 28 &&
    DXGI_FORMAT_R8G8B8A8_UNORM_SRGB == 29 && DXGI_FORMAT_R16G16_FLOAT == 34 && DXGI_FORMAT_R32_FLOAT == 41 &&
    DXGI_FORMAT_B8G8R8A8_UNORM == 87 && DXGI_FORMAT_B8G8R8A8_TYPELESS == 90 && DXGI_FORMAT_B8G8R8A8_UNORM_SRGB == 91);

namespace {
using Microsoft::WRL::ComPtr;
IUnityGraphicsD3D11* unity11 = nullptr;
IUnityGraphicsD3D12v7* unity12 = nullptr;
UnityGfxRenderer renderer = kUnityGfxRendererNull;
// Guards the host services. Enqueue reads the session under it.
std::mutex guard;
rsf_game_host_services services{};
int event_id = 0;
// rsf_unity_d3d11_texture_ops. A queued D3D11 packet's store entry holds one reference to each of
// its textures from admission until its render event ends. Vulkan packets carry image handles,
// which are not reference counted, and are admitted without these leaves.
void retain_texture(void* texture) noexcept { static_cast<IUnknown*>(texture)->AddRef(); }
void release_texture(void* texture) noexcept { static_cast<IUnknown*>(texture)->Release(); }
constexpr rsf_unity_d3d11_texture_ops texture_ops{retain_texture, release_texture};
// A packet the render event took from the store. Its end returns a leased packet's slot and
// releases its references once the event has used them; an unleased packet was freed at take.
struct TakenPacket {
    uint32_t slot = 0;
    bool taken = false;
    TakenPacket() = default;
    TakenPacket(const TakenPacket&) = delete;
    TakenPacket& operator=(const TakenPacket&) = delete;
    ~TakenPacket() { if (taken) rsf_unity_frames_end_packet(slot); }
};
// Holds the record's surface table from the first surface a render event borrows until it returns.
struct SurfaceLease {
    bool held = rsf_unity_d3d11_surfaces_begin();
    SurfaceLease() = default;
    SurfaceLease(const SurfaceLease&) = delete;
    SurfaceLease& operator=(const SurfaceLease&) = delete;
    ~SurfaceLease() { if (held) rsf_unity_d3d11_surfaces_end(); }
};
// Closes a reset list on every early return, so the slot's allocator resets on its next use.
struct Recording {
    ID3D12GraphicsCommandList* list = nullptr;
    ~Recording() { if (list) list->Close(); }
    bool close() { auto* open = list; list = nullptr; return SUCCEEDED(open->Close()); }
};
// A view of the Rust record (d3d11_state.h), borrowed under a frame callback, the creation claim
// or the checked close.
struct DeviceView {
    rsf_unity_d3d11_device_handles handles{};
    ID3D11Device5* device11() const { return static_cast<ID3D11Device5*>(handles.device11); }
    ID3D11DeviceContext4* context11() const { return static_cast<ID3D11DeviceContext4*>(handles.context11); }
    ID3D12Device* device12() const { return static_cast<ID3D12Device*>(handles.device12); }
    ID3D12CommandQueue* queue12() const { return static_cast<ID3D12CommandQueue*>(handles.queue12); }
    ID3D12Fence* completion() const { return static_cast<ID3D12Fence*>(handles.completion); }
    rsf_shared_fence* sharing() const { return static_cast<rsf_shared_fence*>(handles.sharing); }
    ID3D11Fence* fence11() const { return static_cast<ID3D11Fence*>(rsf_shared_fence_d3d11(sharing())); }
    ID3D12Fence* fence12() const { return static_cast<ID3D12Fence*>(rsf_shared_fence_d3d12(sharing())); }
};
// One ring slot's allocator and list, borrowed under the frame callback that records them.
struct SlotView {
    rsf_unity_d3d11_command_handles handles{};
    ID3D12CommandAllocator* allocator() const { return static_cast<ID3D12CommandAllocator*>(handles.allocator); }
    ID3D12GraphicsCommandList* list() const { return static_cast<ID3D12GraphicsCommandList*>(handles.list); }
};
bool active() { return rsf_unity_frames_active() != 0; }
void log(const char* text) { if (services.log) services.log(services.user, text); }
bool wait(const DeviceView& graphics, uint64_t value)
{
    auto* completion = graphics.completion();
    if (!completion) return false;
    const auto completed = completion->GetCompletedValue();
    if (completed == UINT64_MAX) return false;
    if (completed >= value) return true;
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!event) return false;
    const bool ready = SUCCEEDED(completion->SetEventOnCompletion(value, event)) &&
        WaitForSingleObject(event, 10000) == WAIT_OBJECT_0;
    CloseHandle(event);
    if (!ready) return false;
    const auto retired = completion->GetCompletedValue();
    return retired != UINT64_MAX && retired >= value;
}
void barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* resource, D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after)
{
    if (!resource || before == after) return;
    D3D12_RESOURCE_BARRIER change{}; change.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    change.Transition = {resource, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, before, after}; list->ResourceBarrier(1, &change);
}
// rsf_unity_d3d11_device_ops. The create leaves transfer each reference to their output as soon
// as they hold it, so Rust rolls a refused creation back through the destroy leaves.
bool create_shared(const rsf_game_host_services* host, rsf_unity_d3d11_device_handles* output) noexcept try
{
    if (!unity11 || !host) return false;
    // Unity lends its device. The record keeps its own references through release.
    ComPtr<ID3D11Device> device = unity11->GetDevice();
    if (!device) return false;
    ComPtr<ID3D11DeviceContext> context; device->GetImmediateContext(&context);
    ComPtr<ID3D11Device5> device11; ComPtr<ID3D11DeviceContext4> context11;
    if (!context || FAILED(device.As(&device11)) || FAILED(context.As(&context11))) return false;
    output->device11 = device11.Detach();
    output->context11 = context11.Detach();
    rsf_streamline_graphics graphics{}; graphics.struct_size = sizeof(graphics);
    if (rsf_d3d11_present_graphics(&graphics)) {
        // Shared resources report their native owner. Record/execute native lists too,
        // so SR's device identity checks compare the same COM interface throughout.
        ComPtr<ID3D12Device> device12 = static_cast<ID3D12Device*>(graphics.native_device);
        auto* present = rsf_d3d11_present_host();
        void* native_queue = present ? rsf_streamline_host_native(present, graphics.queue) : graphics.queue;
        if (!device12 || !native_queue) return false;
        ComPtr<ID3D12CommandQueue> queue12 = static_cast<ID3D12CommandQueue*>(native_queue);
        output->device12 = device12.Detach();
        output->queue12 = queue12.Detach();
    } else {
        ComPtr<IDXGIDevice> dxgi; ComPtr<IDXGIAdapter> adapter; ComPtr<ID3D12Device> device12;
        if (FAILED(device.As(&dxgi)) || FAILED(dxgi->GetAdapter(&adapter)) ||
            FAILED(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device12)))) return false;
        auto* created = device12.Detach();
        output->device12 = created;
        D3D12_COMMAND_QUEUE_DESC desc{}; ComPtr<ID3D12CommandQueue> queue12;
        if (FAILED(created->CreateCommandQueue(&desc, IID_PPV_ARGS(&queue12)))) return false;
        output->queue12 = queue12.Detach();
    }
    auto* device12 = static_cast<ID3D12Device*>(output->device12);
    rsf_shared_fence* sharing = nullptr;
    const auto shared = rsf_shared_fence_create(output->device11, device12, host->log, host->user, &sharing);
    output->sharing = sharing;
    if (shared != RSF_SHARED_OK) return false;
    ComPtr<ID3D12Fence> completion;
    if (FAILED(device12->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&completion)))) return false;
    output->completion = completion.Detach();
    return true;
} catch (...) { return false; }
// After the slots: the shared fence, then the references in the C++ release order.
void destroy_shared(const rsf_unity_d3d11_device_handles* shared) noexcept
{
    const DeviceView graphics{*shared};
    rsf_shared_fence_destroy(graphics.sharing());
    if (auto* completion = graphics.completion()) completion->Release();
    if (auto* queue = graphics.queue12()) queue->Release();
    if (auto* device = graphics.device12()) device->Release();
    if (auto* context = graphics.context11()) context->Release();
    if (auto* device = graphics.device11()) device->Release();
}
bool create_command_slot(const rsf_unity_d3d11_device_handles* shared, rsf_unity_d3d11_command_handles* output) noexcept
{
    auto* device = DeviceView{*shared}.device12();
    ComPtr<ID3D12CommandAllocator> allocator;
    if (FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&allocator)))) return false;
    auto* owned = allocator.Detach();
    output->allocator = owned;
    ComPtr<ID3D12GraphicsCommandList> list;
    if (FAILED(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, owned, nullptr, IID_PPV_ARGS(&list)))) return false;
    auto* commands = list.Detach();
    output->list = commands;
    return SUCCEEDED(commands->Close());
}
void destroy_command_slot(const rsf_unity_d3d11_command_handles* slot) noexcept
{
    const SlotView view{*slot};
    if (auto* list = view.list()) list->Release();
    if (auto* allocator = view.allocator()) allocator->Release();
}
// The checked close's fence reads. Neither waits.
uint64_t completion_value(const rsf_unity_d3d11_device_handles* shared) noexcept
{
    auto* completion = DeviceView{*shared}.completion();
    return completion ? completion->GetCompletedValue() : 0;
}
uint64_t shared_value(const rsf_unity_d3d11_device_handles* shared) noexcept
{
    auto* fence = DeviceView{*shared}.fence11();
    return fence ? fence->GetCompletedValue() : 0;
}
// Surface replacement's retirement wait: the completion fence, as the C++ replacement waited.
bool wait_completion(const rsf_unity_d3d11_device_handles* shared, uint64_t value) noexcept
{
    return wait(DeviceView{*shared}, value);
}
// A render-target surface on both devices, as the C++ replacement created it.
bool create_surface(const rsf_unity_d3d11_device_handles* shared, const rsf_game_host_services* host,
    uint32_t width, uint32_t height, uint32_t format, void** output) noexcept
{
    const DeviceView graphics{*shared};
    rsf_shared_surface_setup setup{sizeof(setup), RSF_SHARED_SURFACE_ABI_VERSION, width, height, format, 1,
        host ? host->log : nullptr, host ? host->user : nullptr, 0};
    rsf_shared_surface* created = nullptr;
    const auto result = rsf_shared_surface_create(graphics.device11(), graphics.device12(), &setup, &created);
    *output = created;
    return result == RSF_SHARED_OK;
}
void destroy_surface(void* surface) noexcept { rsf_shared_surface_destroy(static_cast<rsf_shared_surface*>(surface)); }
constexpr rsf_unity_d3d11_device_ops device_ops{
    create_shared, destroy_shared, create_command_slot, destroy_command_slot, completion_value, shared_value,
    wait_completion, create_surface, destroy_surface,
};
// The caller holds a frame callback. Only the attempt that failed reports, once until release.
bool initialize11(DeviceView& graphics)
{
    const uint32_t ready = rsf_unity_d3d11_initialize(&services, &device_ops, &graphics.handles);
    if (ready == RSF_UNITY_D3D11_DEVICE_FAILED)
        rsf_unity_package_bridge_error(RSF_BACKEND_ERROR_INIT_FAILED, "Unity DX11 shared device/fence initialization failed; native output remains in place.");
    return ready == RSF_UNITY_D3D11_DEVICE_READY;
}
// The caller holds the surface lease. The record keys surface index by the source description,
// chooses the shared format and replaces the surface after the ring retires; this leaf only reads
// the description.
rsf_shared_surface* prepare_surface(uint32_t index, ID3D11Texture2D* texture)
{
    if (!texture) return nullptr;
    D3D11_TEXTURE2D_DESC desc{}; texture->GetDesc(&desc);
    void* surface = nullptr;
    if (!rsf_unity_d3d11_prepare_surface(index, &services, desc.Width, desc.Height,
            static_cast<uint32_t>(desc.Format), &surface)) return nullptr;
    return static_cast<rsf_shared_surface*>(surface);
}
bool retire_interop_submission(const DeviceView& graphics, uint64_t value, bool handed_back)
{
    // Queue work has already gone out. Publish the retirement fence even if the D3D11
    // wait or shared-fence signal refused, so release still waits for the submitted work.
    const bool signalled = value && SUCCEEDED(graphics.queue12()->Signal(graphics.completion(), value));
    // Rust stops the adapter on either failure before the log, as the C++ did.
    switch (rsf_unity_d3d11_retire(signalled, handed_back)) {
    case RSF_UNITY_D3D11_RETIRED: return true;
    case RSF_UNITY_D3D11_HANDBACK_FAILED: log("Unity DX11 shared-surface hand-back failed; adapter stopped."); return false;
    default: log("Unity DX11 completion signal failed; adapter stopped."); return false;
    }
}
bool signal11(const DeviceView& graphics, ID3D11Fence* fence, uint64_t value)
{
    if (!value || FAILED(graphics.context11()->Signal(fence, value))) return false;
    graphics.context11()->Flush();
    rsf_unity_interop_d3d11_submitted(value);
    return true;
}
// Runs under render_event's frame callback, which keeps the borrowed record alive.
void execute11(int id, const rsf_unity_packet& packet)
{
    DeviceView graphics;
    if (!active() || !unity11 || !initialize11(graphics) || !services.render_pass) return;
    auto* context11 = graphics.context11();
    auto* queue12 = graphics.queue12();
    auto* fence11 = graphics.fence11();
    auto* fence12 = graphics.fence12();
    // Route, pass, surface slots and states (d3d11_pass.rs). render_event admitted the id range.
    rsf_unity_d3d11_pass_plan plan{};
    if (!rsf_unity_d3d11_plan_pass(static_cast<uint32_t>(id - event_id), &packet, rsf_unity_package_is_editor(), &plan)) return;
    rsf_game_render_pass& pass = plan.pass;
    if (plan.route == RSF_UNITY_D3D11_ROUTE_WINDOW || plan.route == RSF_UNITY_D3D11_ROUTE_PREVIEW) {
        pass.swapchain = unity11->GetSwapChain();
        if (plan.route == RSF_UNITY_D3D11_ROUTE_PREVIEW) {
            SurfaceLease lease;
            ComPtr<ID3D11Texture2D> source;
            if (!lease.held || FAILED(static_cast<IUnknown*>(packet.color)->QueryInterface(IID_PPV_ARGS(&source)))) return;
            rsf_shared_surface* preview = prepare_surface(plan.surfaces[0], source.Get());
            if (!preview) return;
            rsf_unity_interop_record_d3d11_copy();
            context11->CopyResource(static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(preview)), source.Get());
            uint64_t uploaded = 0, consumed = 0;
            if (!rsf_unity_interop_shared_pair(&uploaded, &consumed) || !signal11(graphics, fence11, uploaded)) return;
            if (FAILED(queue12->Wait(fence12, uploaded))) return;
            // The plan carries RSF_UNITY_PREVIEW_SOURCE_COMMON for this colour.
            pass.color_input = rsf_shared_surface_d3d12(preview);
            services.render_pass(services.user, queue12, &pass, 1);
            const bool handed_back = SUCCEEDED(queue12->Signal(fence12, consumed)) &&
                SUCCEEDED(context11->Wait(fence11, consumed));
            (void)retire_interop_submission(graphics, rsf_unity_interop_completion(), handed_back);
            return;
        }
        services.render_pass(services.user, queue12, &pass, 1); return;
    }
    // Held through the output copy and the closing render-pass call, which read the surfaces.
    SurfaceLease lease;
    if (!lease.held) return;
    uint64_t retired = 0;
    const uint32_t index = rsf_unity_interop_slot(&retired);
    SlotView slot;
    if (!rsf_unity_d3d11_command_slot(index, &slot.handles)) return;
    if (!wait(graphics, retired)) { log("Unity DX11 command-slot retirement timed out."); return; }
    if (FAILED(slot.allocator()->Reset()) || FAILED(slot.list()->Reset(slot.allocator(), nullptr))) return;
    Recording recording{slot.list()};
    std::array<ComPtr<ID3D11Texture2D>, 4> input;
    std::array<rsf_shared_surface*, 4> shared{};
    void* pointers[] = {packet.color, packet.depth, packet.motion, packet.output};
    if (plan.swapchain_color) {
        auto* chain = unity11->GetSwapChain();
        if (!chain || FAILED(chain->GetBuffer(0, IID_PPV_ARGS(&input[0])))) return;
    }
    for (uint32_t i = 0; i < 4; ++i) {
        if (pointers[i] && FAILED(static_cast<IUnknown*>(pointers[i])->QueryInterface(IID_PPV_ARGS(&input[i])))) return;
        if (!input[i]) continue;
        shared[i] = prepare_surface(plan.surfaces[i], input[i].Get());
        if (!shared[i]) return;
        rsf_unity_interop_record_d3d11_copy();
        context11->CopyResource(static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(shared[i])), input[i].Get());
    }
    uint64_t upload = 0, download = 0;
    if (!rsf_unity_interop_shared_pair(&upload, &download) || !signal11(graphics, fence11, upload)) return;
    if (FAILED(queue12->Wait(fence12, upload))) return;
    std::array<ID3D12Resource*, 4> resources{};
    for (uint32_t i = 0; i < 4; ++i) if (input[i]) {
        resources[i] = static_cast<ID3D12Resource*>(rsf_shared_surface_d3d12(shared[i]));
        barrier(slot.list(), resources[i], D3D12_RESOURCE_STATE_COMMON, static_cast<D3D12_RESOURCE_STATES>(plan.states[i]));
    }
    pass.color_input = resources[0]; pass.depth = resources[1]; pass.motion = resources[2]; pass.color_output = resources[3];
    // A policy change applies on the next reconstruction once every submission completed; it
    // stays pending, and config keeps the bridge disabled, until then.
    if (!rsf_unity_d3d11_apply_change(&plan)) return;
    rsf_unity_package_sr_slot_reset(11, slot.list(), index);
    services.render_pass(services.user, slot.list(), &pass, 1);
    for (uint32_t i = 0; i < 4; ++i) if (resources[i])
        barrier(slot.list(), resources[i], static_cast<D3D12_RESOURCE_STATES>(plan.states[i]), D3D12_RESOURCE_STATE_COMMON);
    if (!recording.close()) return;
    ID3D12CommandList* lists[] = {slot.list()}; queue12->ExecuteCommandLists(1, lists);
    const bool handed_back = SUCCEEDED(queue12->Signal(fence12, download)) &&
        SUCCEEDED(context11->Wait(fence11, download));
    if (handed_back && input[3] && plan.copy_back) {
        rsf_unity_interop_record_d3d11_copy();
        context11->CopyResource(input[3].Get(), static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(shared[3])));
        if (!signal11(graphics, fence11, rsf_unity_interop_d3d11_tail())) {
            rsf_unity_frames_activate(0);
            log("Unity DX11 output copy has no completion signal; adapter stopped and remains owned.");
        }
    }
    if (!retire_interop_submission(graphics, rsf_unity_interop_submit(index), handed_back)) return;
    services.render_pass(services.user, slot.list(), &pass, 0);
    rsf_unity_interop_advance(index);
}
void UNITY_INTERFACE_API render_event(int id, void* address) noexcept try
{
    rsf_unity_frame_callback call;
    if (!call.entered) return;
    if (id < event_id || id > event_id + 2) return;
    rsf_unity_packet packet{};
    // Ends before the frame callback leaves, as the native leases did.
    TakenPacket taken;
    taken.taken = rsf_unity_frames_take_packet(address, &packet, &taken.slot) != 0;
    if (!taken.taken) return;
    if (!active()) return;
    if (renderer == kUnityGfxRendererVulkan) {
        uint32_t change = 0;
        const bool reconfigure = id == event_id && rsf_unity_frames_pending_change(&change);
        if (rsf_uvk_render(id, packet, services, reconfigure) && reconfigure) rsf_unity_frames_settle_change(change);
    }
    else execute11(id, packet);
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
    rsf_game_cpu_event event{sizeof(event), stage, services.session_id, frame, static_cast<uint64_t>(now.QuadPart), static_cast<uint64_t>(frequency.QuadPart), 0, 0, 0};
    event.event_version=RSF_GAME_CPU_EVENT_VERSION; event.flags=RSF_GAME_CPU_INPUT_KEYED;
    services.cpu_event(services.user, &event);
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
    if (renderer == kUnityGfxRendererD3D11) return rsf_unity_d3d11_admit_packet(packet, services.session_id, &texture_ops);
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
    if (renderer == kUnityGfxRendererD3D12) { rsf_unity_d3d12_release(); return; }
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
        DeviceView graphics;
        if (!call.entered || !rsf_unity_d3d11_device_view(&graphics.handles)) return false;
        owning_device = graphics.device12(); owning_queue = graphics.queue12();
        return true;
    }
    return renderer == kUnityGfxRendererVulkan && rsf_uvk_graphics(owning_device, owning_queue);
}
