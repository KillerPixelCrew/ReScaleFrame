// SPDX-License-Identifier: MIT
// The shared Unity D3D12 render bridge, linked into the game plugin and the Unity package. Its
// frame state (packets, policy generation, ring admission, sources, retirement) is Rust
// (bridge_state.rs); this file keeps the Unity interfaces, D3D12 objects and resource leases,
// render-pass calls, CPU input events and the exported entry points.
#include "bridge.h"
#include "bridge_state.h"
#include <rescaleframe/sha256_file.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <array>
#include <atomic>
#include <cstring>
#include <cstdio>
#include <mutex>
#if RSF_HAVE_UNITY_NATIVE
#include <IUnityGraphics.h>
#include <IUnityGraphicsD3D12.h>
#endif

namespace {
using Microsoft::WRL::ComPtr;
using Leases = std::array<ComPtr<ID3D12Resource>, 4>;
// A packet taken from the Rust store with the leases enqueue acquired for it.
struct Pending {
    rsf_unity_packet packet{};
    Leases resources;
};
struct Commands {
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;
    Leases leases;
};
// Guards the host services and the packet leases, and orders them with the Rust packet store.
std::mutex guard;
rsf_game_host_services services{};
std::array<Leases, RSF_UNITY_FRAMES_PACKETS> pending_leases;
std::array<Commands, RSF_UNITY_FRAMES_SLOTS> commands;
ComPtr<ID3D12Fence> fence;
uint32_t refusals = 0;
int event_id = 0;
bool active() { return rsf_unity_frames_active() != 0; }
// Unity's frame-fence completion, or 0 without a fence: every recorded submission is then pending.
uint64_t completed() { return fence ? fence->GetCompletedValue() : 0; }
#if RSF_HAVE_UNITY_NATIVE
IUnityGraphics* graphics = nullptr;
IUnityGraphicsD3D12v7* unity = nullptr;
bool player_registry_matches(HMODULE player)
{
    constexpr unsigned char bytes[] = {0x48,0x83,0xec,0x38,0x48,0x89,0x4c,0x24,0x20,0x48,0x8d,0x4c,0x24,0x20,0x48,0x89,0x54,0x24,0x28,0xe8,0x78,0xff,0xff,0xff};
    auto* site = reinterpret_cast<unsigned char*>(player) + 0x74fc80;
    if (std::memcmp(site, bytes, sizeof(bytes))) return false;
    wchar_t path[1024]{};
    if (!GetModuleFileNameW(player, path, 1024)) return false;
    unsigned char digest[32]{};
    if (!rsf::sha256_file(path, digest)) return false;
    constexpr unsigned char expected[] = {0xbb,0xdf,0x3e,0x73,0x52,0x22,0x23,0xea,0x45,0xc3,0xa5,0x40,0x49,0xb7,0x16,0xf9,0xda,0x3e,0x63,0x29,0x94,0x63,0x67,0x34,0xc5,0xd5,0x33,0x26,0xaa,0x2c,0x36,0xd0};
    return !std::memcmp(digest, expected, 32);
}
void configure_graphics()
{
    if (!graphics || !unity || graphics->GetRenderer() != kUnityGfxRendererD3D12) { unity = nullptr; return; }
    event_id = graphics->ReserveEventIDRange(3);
    UnityD3D12PluginEventConfig event{};
    event.graphicsQueueAccess = kUnityD3D12GraphicsQueueAccess_DontCare;
    event.flags = kUnityD3D12EventConfigFlag_SyncWorkerThreads | kUnityD3D12EventConfigFlag_FlushCommandBuffers;
    unity->ConfigureEvent(event_id, &event);
    event.graphicsQueueAccess = kUnityD3D12GraphicsQueueAccess_Allow;
    unity->ConfigureEvent(event_id + 1, &event);
    // Keep the pre-UI capture on Unity's recording list, rather than submit a separate list
    // whose worker execution can slip past the UI and direct-queue overlay draws.
    event.graphicsQueueAccess = kUnityD3D12GraphicsQueueAccess_DontCare;
    event.flags = kUnityD3D12EventConfigFlag_SyncWorkerThreads | kUnityD3D12EventConfigFlag_ModifiesCommandBuffersState;
    event.ensureActiveRenderTextureIsBound = true;
    unity->ConfigureEvent(event_id + 2, &event);
}
void make_pass(const rsf_unity_packet& packet, rsf_game_render_pass& pass)
{
    pass = {}; pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_SR;
    pass.session_id = packet.session_id; pass.family_key = pass.view_key = packet.view_key;
    pass.history_key = packet.view_key; pass.native_frame = packet.frame_id; pass.resource_generation = packet.generation;
    pass.source_frame_id = packet.frame_id;
    // Unity has no screen classifier yet, so every camera pass counts as gameplay for FG.
    pass.screen = RSF_SCREEN_FLIGHT;
    pass.flags = packet.flags & RSF_UNITY_PACKET_RESET; pass.color_input = packet.color; pass.color_output = packet.output;
    pass.depth = packet.depth; pass.motion = packet.motion; pass.camera = packet.camera;
    pass.camera_valid = (packet.flags & RSF_UNITY_PACKET_PROBE) && !(packet.flags & RSF_UNITY_PACKET_INPUTS_ONLY) ? 0u : 1u;
    if (packet.flags & RSF_UNITY_PACKET_INPUTS_ONLY) pass.role = RSF_GAME_RENDER_FG_INPUTS;
    pass.render_rect[2] = static_cast<int32_t>(packet.camera.render_width);
    pass.render_rect[3] = static_cast<int32_t>(packet.camera.render_height);
    pass.output_rect[2] = static_cast<int32_t>(packet.camera.output_width);
    pass.output_rect[3] = static_cast<int32_t>(packet.camera.output_height);
    pass.motion_to_uv[0] = pass.motion_to_uv[1] = 1; pass.motion_camera_included = 1;
    std::memcpy(pass.previous_clip_to_clip, packet.previous_clip_to_clip, sizeof(pass.previous_clip_to_clip));
}
#endif

void log(const char* message)
{
    if (services.log) services.log(services.user, message);
}
#if RSF_HAVE_UNITY_NATIVE
rsf_unity_source_submission source_of(const rsf_game_render_pass& pass)
{
    return {pass.session_id, pass.source_frame_id, pass.view_key};
}

void confirm_flushed_sources(ID3D12CommandQueue* queue, const rsf_game_render_pass& window)
{
    if (!queue) return;
    std::array<rsf_unity_source_submission, RSF_UNITY_FRAMES_SOURCES> flushed{};
    const auto count = rsf_unity_frames_take_flushed(window.session_id, window.source_frame_id, flushed.data());
    for (uint32_t i = 0; i < count; ++i) {
        const auto& source = flushed[i];
        rsf_game_render_pass submission{};
        submission.struct_size = sizeof(submission);
        submission.role = RSF_GAME_RENDER_SUBMISSION;
        submission.session_id = source.session_id;
        submission.source_frame_id = source.source_frame_id;
        submission.view_key = source.view_key;
        services.render_pass(services.user, queue, &submission, 0);
    }
}
#endif
int config(uint32_t width, uint32_t height, rsf_unity_config* output)
{
    return rsf_unity_frames_config(&services, width, height, output);
}
void* enqueue(const rsf_unity_packet* packet)
{
    std::lock_guard<std::mutex> lock(guard);
    uint32_t slot = 0;
    void* stored = rsf_unity_frames_admit_packet(packet, services.session_id, &slot);
    if (!stored) return nullptr;
    // Lease the resources before the stored address reaches Unity's render event.
    void* textures[] = {packet->color, packet->depth, packet->motion, packet->output};
    auto& leases = pending_leases[slot];
    for (size_t i = 0; i < leases.size(); ++i) leases[i] = static_cast<ID3D12Resource*>(textures[i]);
    return stored;
}
void __stdcall render(int id, void* address) noexcept try
{
    if (id != event_id && id != event_id + 1 && id != event_id + 2) return;
    rsf_unity_frames_callback_enter();
    struct Finish { ~Finish() { rsf_unity_frames_callback_leave(); } } finish;
    Pending entry;
    {
        std::lock_guard<std::mutex> lock(guard);
        uint32_t slot = 0;
        if (!rsf_unity_frames_take_packet(address, &entry.packet, &slot)) return;
        entry.resources = std::move(pending_leases[slot]);
        pending_leases[slot] = {};
    }
#if RSF_HAVE_UNITY_NATIVE
    if (!active() || !unity || !services.render_pass) return;
    ID3D12Device* device = unity->GetDevice();
    if (!device) return;
    if (id == event_id + 2 && (entry.packet.flags & RSF_UNITY_PACKET_HUDLESS)) {
        if (!fence) fence = unity->GetFrameFence();
        const auto completion = unity->GetNextFrameFenceValue();
        if (!fence || !completion || completion == UINT64_MAX || completed() == UINT64_MAX) return;
        UnityGraphicsD3D12RecordingState recording{};
        if (!unity->CommandRecordingState(&recording) || !recording.commandList) {
            if (++refusals <= 3) log("Unity pre-UI capture refused: no current recording list.");
            return;
        }
        if (!entry.packet.color) {
            ComPtr<IDXGISwapChain3> chain;
            auto* owned = unity->GetSwapChain();
            if (!owned || FAILED(owned->QueryInterface(IID_PPV_ARGS(&chain))) ||
                FAILED(chain->GetBuffer(chain->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&entry.resources[0])))) return;
            entry.packet.color = entry.resources[0].Get();
        }
        rsf_game_render_pass pass{}; make_pass(entry.packet, pass);
        pass.role = RSF_GAME_RENDER_FINAL_SCENE;
        services.render_pass(services.user, recording.commandList, &pass, 1);
        const auto recorded = source_of(pass);
        rsf_unity_frames_record_source(&recorded, completion);
        static uint32_t reports = 0;
        if (reports++ < 3) {
            const auto source = entry.resources[0]->GetDesc();
            char text[160]; std::snprintf(text, sizeof(text), "Unity pre-UI copy recorded on engine list: format=%u %llux%u", source.Format, source.Width, source.Height);
            log(text);
        }
        return;
    }
    if (id == event_id + 1 && (entry.packet.flags & RSF_UNITY_PACKET_WINDOW)) {
        rsf_game_render_pass pass{}; make_pass(entry.packet, pass);
        pass.role = RSF_GAME_RENDER_WINDOW; pass.swapchain = unity->GetSwapChain();
        auto* queue = unity->GetCommandQueue();
        // This configured event synchronizes workers, flushes Unity's command lists,
        // and grants exclusive queue access. Recording and worker tickets precede it.
        confirm_flushed_sources(queue, pass);
        services.render_pass(services.user, queue, &pass, 1);
        return;
    }
    if (!fence) fence = unity->GetFrameFence();
    if (!fence) return;
    const bool hudless = (entry.packet.flags & RSF_UNITY_PACKET_HUDLESS) != 0;
    // Extent and policy changes wait for the ring; a busy slot leaves the queued spatial fallback.
    rsf_unity_frame_admission admission{};
    if (!rsf_unity_frames_admit_frame(&entry.packet, completed(), &admission)) return;
    const bool changing = admission.reconfigure != 0;
    auto& slot = commands[admission.slot];
    slot.leases = {};
    if (!slot.allocator || !slot.list) {
        ComPtr<ID3D12CommandAllocator> allocator;
        ComPtr<ID3D12GraphicsCommandList> list;
        if (FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&allocator))) ||
            FAILED(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator.Get(), nullptr, IID_PPV_ARGS(&list))) ||
            FAILED(list->Close())) return;
        slot.allocator = std::move(allocator); slot.list = std::move(list);
    }
    ComPtr<ID3D12Device> owner;
    if (FAILED(slot.allocator->GetDevice(IID_PPV_ARGS(&owner))) || owner.Get() != device) {
        rsf_unity_frames_activate(0); log("Unity D3D12 device changed; adapter requires a drained restart."); return;
    }
    if (FAILED(slot.allocator->Reset()) || FAILED(slot.list->Reset(slot.allocator.Get(), nullptr))) return;
    rsf_game_render_pass pass{}; make_pass(entry.packet, pass);
    if (hudless) pass.role = RSF_GAME_RENDER_FINAL_SCENE;
    if (changing) { pass.flags |= RSF_GAME_RENDER_RECONFIGURE; pass.camera_valid = 0; }
    services.render_pass(services.user, slot.list.Get(), &pass, 1);
    if (FAILED(slot.list->Close())) return;
    std::array<UnityGraphicsD3D12ResourceState, 4> states{};
    int count = 0;
    for (size_t i = 0; i < 4; ++i) if (entry.resources[i]) {
        auto state = hudless ? D3D12_RESOURCE_STATE_RENDER_TARGET : i == 3 ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        states[static_cast<size_t>(count++)] = {entry.resources[i].Get(), state, state};
    }
    slot.leases = std::move(entry.resources);
    const auto complete = unity->ExecuteCommandList(slot.list.Get(), count, states.data());
    // Missing completion proof stops the bridge and keeps the command allocator owned.
    const auto source = source_of(pass);
    if (!rsf_unity_frames_submitted(&admission, complete, &source))
        log("Unity D3D12 submission returned no valid completion; adapter stopped and remains owned.");
#else
    (void)entry;
#endif
}
catch (...) { if (++refusals <= 3) log("Unity render callback refused; spatial fallback remains queued."); }
}

namespace {

std::atomic<uint64_t> input_source{};
std::atomic<uint32_t> input_calls{};
struct InputCall { InputCall() { ++input_calls; } ~InputCall() { --input_calls; } };
HHOOK message_hook{};
void received_input(uint32_t kind,uint32_t message) {
    if (!active() || !services.cpu_event) return;
    const auto source=input_source.load();
    LARGE_INTEGER now{},frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    const rsf_game_cpu_event fact{sizeof(fact),RSF_GAME_CPU_INPUT_EVENT,services.session_id,source,
        uint64_t(now.QuadPart),uint64_t(frequency.QuadPart),0,kind,message,RSF_GAME_CPU_EVENT_VERSION,
        RSF_GAME_CPU_INPUT_KEYED | (message ? RSF_GAME_CPU_WINDOW_MESSAGE : 0u) | (!source && kind ? RSF_GAME_CPU_INPUT_UNFRAMED : 0u)};
    services.cpu_event(services.user,&fact);
}
LRESULT CALLBACK received_message(int code,WPARAM removed,LPARAM argument) {
    InputCall call;
    if (code>=0 && removed==PM_REMOVE) {
        const auto& message=*reinterpret_cast<const MSG*>(argument); uint32_t kind=0;
        if (message.message>=WM_KEYFIRST && message.message<=WM_KEYLAST) kind=RSF_GAME_INPUT_KEYBOARD;
        if (message.message>=WM_MOUSEFIRST && message.message<=WM_MOUSELAST) kind=RSF_GAME_INPUT_MOUSE;
        if (message.message==WM_INPUT) {
            RAWINPUTHEADER header{}; UINT bytes=sizeof(header);
            if (GetRawInputData(reinterpret_cast<HRAWINPUT>(message.lParam),RID_HEADER,&header,&bytes,sizeof(header))==sizeof(header))
                kind=header.dwType==RIM_TYPEKEYBOARD ? RSF_GAME_INPUT_KEYBOARD : header.dwType==RIM_TYPEMOUSE ? RSF_GAME_INPUT_MOUSE : 0;
        }
        received_input(kind,message.message);
    }
    return CallNextHookEx(nullptr,code,removed,argument);
}
void cpu_fact(const rsf_game_cpu_event* fact) {
    InputCall call;
    if (!active() || !fact || fact->struct_size<sizeof(*fact) || fact->session_id!=services.session_id || !services.cpu_event) return;
    auto copy=*fact;
    if (copy.stage==RSF_GAME_CPU_INPUT_EVENT) {
        copy.source_frame_id=input_source.load();
        if (!copy.source_frame_id) copy.flags|=RSF_GAME_CPU_INPUT_UNFRAMED;
    }
    services.cpu_event(services.user,&copy);
}
void cpu_event(uint32_t stage, uint64_t frame_id) {
    InputCall call;
    if (!active() || !frame_id || !services.cpu_event) return;
    LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    rsf_game_cpu_event event{}; event.struct_size = sizeof(event); event.stage = stage;
    event.session_id = services.session_id; event.source_frame_id = frame_id;
    event.timestamp_qpc = uint64_t(now.QuadPart); event.qpc_frequency = uint64_t(frequency.QuadPart);
    event.event_version=RSF_GAME_CPU_EVENT_VERSION; event.flags=RSF_GAME_CPU_INPUT_KEYED;
    if (stage==RSF_GAME_CPU_FRAME_BEGIN) {
        input_source.store(frame_id);
        if (!message_hook) message_hook=SetWindowsHookExW(WH_GETMESSAGE,received_message,nullptr,GetCurrentThreadId());
    }
    services.cpu_event(services.user, &event);
    if (stage==RSF_GAME_CPU_FRAME_END) input_source.store(0);
}
}
bool rsf_unity_bridge_prepare(const rsf_game_host_services& host, rsf_unity_native_api* api) noexcept
{
    if (!api || !host.render_pass || !host.render_config || !host.log) return false;
    std::lock_guard<std::mutex> lock(guard);
    services = host;
    *api = {sizeof(*api), RSF_UNITY_NATIVE_ABI_VERSION, host.session_id, log, config, enqueue,
        reinterpret_cast<void*>(render), rsf_unity_frames_report_managed, cpu_event, cpu_fact};
    return true;
}
void rsf_unity_bridge_activate(bool enabled) noexcept { rsf_unity_frames_activate(enabled ? 1u : 0u); }
bool rsf_unity_bridge_drained() noexcept
{
    std::lock_guard<std::mutex> lock(guard);
    if (input_calls.load()) return false;
    return rsf_unity_frames_drained(completed()) != 0;
}
void rsf_unity_bridge_release() noexcept
{
    std::lock_guard<std::mutex> lock(guard);
    if (message_hook) {UnhookWindowsHookEx(message_hook);message_hook=nullptr;} input_source.store(0);
    commands = {}; pending_leases = {}; fence.Reset(); services = {};
    rsf_unity_frames_release();
}
uint32_t rsf_unity_bridge_managed_state() noexcept { return rsf_unity_frames_managed_stage(); }
extern "C" __declspec(dllexport) void* rsf_unity_get_render_event() noexcept
{
#if RSF_HAVE_UNITY_NATIVE
    if (!unity) {
        HMODULE player = GetModuleHandleW(L"UnityPlayer.dll");
        if (player && player_registry_matches(player)) {
            using Lookup = IUnityInterface* (__stdcall*)(uint64_t, uint64_t);
            auto lookup = reinterpret_cast<Lookup>(reinterpret_cast<unsigned char*>(player) + 0x74fc80);
            const auto g = GetUnityInterfaceGUID<IUnityGraphics>();
            const auto d = GetUnityInterfaceGUID<IUnityGraphicsD3D12v7>();
            graphics = static_cast<IUnityGraphics*>(lookup(g.m_GUIDHigh, g.m_GUIDLow));
            unity = static_cast<IUnityGraphicsD3D12v7*>(lookup(d.m_GUIDHigh, d.m_GUIDLow));
            configure_graphics();
            if (unity) log("Unity native registry matched through player PDB, SHA256 and expected-byte guards.");
        }
    }
    return unity ? reinterpret_cast<void*>(render) : nullptr;
#else
    return nullptr;
#endif
}
extern "C" __declspec(dllexport) int rsf_unity_get_event_id() noexcept { return event_id; }
extern "C" __declspec(dllexport) int rsf_unity_set_engine_spatial(uint32_t requested_engine_spatial, uint32_t requested_quality) noexcept
{
    return rsf_unity_frames_set_policy(requested_engine_spatial, requested_quality);
}
#if RSF_HAVE_UNITY_NATIVE
extern "C" void UNITY_INTERFACE_API UnityPluginLoad(IUnityInterfaces* interfaces)
{
    if (!interfaces) return;
    graphics = interfaces->Get<IUnityGraphics>();
    if (!graphics || graphics->GetRenderer() != kUnityGfxRendererD3D12) return;
    unity = interfaces->Get<IUnityGraphicsD3D12v7>();
    if (!unity) return;
    configure_graphics();
}
extern "C" void UNITY_INTERFACE_API UnityPluginUnload()
{
    rsf_unity_frames_activate(0);
    // Callback/device retirement is coordinated by the lifecycle before explicit unload.
    unity = nullptr; graphics = nullptr;
}
#endif
