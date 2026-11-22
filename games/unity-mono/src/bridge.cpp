// SPDX-License-Identifier: MIT
// Typed Unity and D3D12 calls for the Mono plugin and the Unity package. Rust owns frame state,
// packet leases, pass fields and CPU stage facts. The native leaf retains typed graphics
// references through Unity's frame fence and reports list recording and queue submission.
#include "bridge.h"
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
#if defined(RSF_UNITY_PACKAGE_BRIDGE)
extern "C" void rsf_unity_package_sr_slot_reset(uint32_t bridge, void* commands, uint32_t slot);
#endif

namespace {
using Microsoft::WRL::ComPtr;
using Leases = std::array<ComPtr<ID3D12Resource>, 4>;
struct Commands {
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;
    Leases leases;
};
// Orders host preparation and shutdown with packet publication.
std::mutex guard;
#if RSF_HAVE_UNITY_NATIVE
struct RenderEventScope {
    const bool entered = rsf_unity_frames_begin_d3d12_event();
    ~RenderEventScope() { if (entered) rsf_unity_frames_end_d3d12_event(); }
};
#endif
rsf_game_host_services services{};
void retain_texture(void* texture) { static_cast<ID3D12Resource*>(texture)->AddRef(); }
void release_texture(void* texture) { static_cast<ID3D12Resource*>(texture)->Release(); }
constexpr rsf_unity_texture_ops texture_ops{retain_texture, release_texture};
std::array<Commands, RSF_UNITY_FRAMES_SLOTS> commands;
ComPtr<ID3D12Fence> fence;
std::atomic<uint32_t> refusals{0};
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

#endif
int config(uint32_t width, uint32_t height, rsf_unity_config* output)
{
    rsf_unity_frame_callback call;
    if (!call.entered) return 0;
    return rsf_unity_frames_config(&services, width, height, output);
}
void* enqueue(const rsf_unity_packet* packet)
{
    std::lock_guard<std::mutex> lock(guard);
    return rsf_unity_frames_admit_leased_packet(packet, services.session_id, &texture_ops);
}
void __stdcall render(int id, void* address) noexcept try
{
    rsf_unity_frame_callback call;
    if (!call.entered) return;
    if (id != event_id && id != event_id + 1 && id != event_id + 2) return;
    rsf_unity_taken_packet entry{address};
    if (!entry.taken) return;
#if RSF_HAVE_UNITY_NATIVE
    Leases resources;
    RenderEventScope render_scope;
    if (!render_scope.entered || !active() || !unity || !services.render_pass) return;
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
                FAILED(chain->GetBuffer(chain->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&resources[0])))) return;
            entry.packet.color = resources[0].Get();
        }
        rsf_game_render_pass pass{}; rsf_unity_build_d3d12_pass(&entry.packet, &pass);
        pass.role = RSF_GAME_RENDER_FINAL_SCENE;
        const auto recorded = source_of(pass);
        if (!rsf_unity_frames_can_record_source(&recorded)) return;
        services.render_pass(services.user, recording.commandList, &pass, 1);
        rsf_unity_frames_record_source(&recorded, completion);
        static uint32_t reports = 0;
        if (reports++ < 3) {
            const auto source = static_cast<ID3D12Resource*>(entry.packet.color)->GetDesc();
            char text[160]; std::snprintf(text, sizeof(text), "Unity pre-UI copy recorded on engine list: format=%u %llux%u", source.Format, source.Width, source.Height);
            log(text);
        }
        return;
    }
    if (id == event_id + 1 && (entry.packet.flags & RSF_UNITY_PACKET_WINDOW)) {
        rsf_game_render_pass pass{}; rsf_unity_build_d3d12_pass(&entry.packet, &pass);
        pass.role = RSF_GAME_RENDER_WINDOW; pass.swapchain = unity->GetSwapChain();
        auto* queue = unity->GetCommandQueue();
        // This configured event synchronizes workers, flushes Unity's command lists,
        // and grants exclusive queue access. Recording and worker tickets precede it.
        rsf_unity_frames_flush_sources(&services, queue, &pass);
        services.render_pass(services.user, queue, &pass, 1);
        return;
    }
    if (!fence) fence = unity->GetFrameFence();
    if (!fence) return;
    const bool hudless = (entry.packet.flags & RSF_UNITY_PACKET_HUDLESS) != 0;
    // Extent and policy changes wait for the ring; a busy slot leaves the queued spatial fallback.
    rsf_unity_frame_admission admission{};
    if (!rsf_unity_frames_admit_frame(&entry.packet, completed(), &admission)) return;
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
#if defined(RSF_UNITY_PACKAGE_BRIDGE)
    // Admission checked Unity's prior submission fence. A successful reset also proves discard
    // of a preceding recording that never reached ExecuteCommandList.
    rsf_unity_package_sr_slot_reset(12, slot.list.Get(), admission.slot);
#endif
    rsf_game_render_pass pass{}; rsf_unity_build_d3d12_pass(&entry.packet, &pass);
    if (hudless) pass.role = RSF_GAME_RENDER_FINAL_SCENE;
    if (!rsf_unity_frames_dispatch_d3d12(&services, slot.list.Get(), &pass, admission.pending_generation)) {
        (void)slot.list->Close();
        return;
    }
    if (FAILED(slot.list->Close())) return;
    // The packet lease covers recording. These references extend through GPU completion.
    void* textures[] = {entry.packet.color, entry.packet.depth, entry.packet.motion, entry.packet.output};
    for (size_t i = 0; i < resources.size(); ++i)
        resources[i] = static_cast<ID3D12Resource*>(textures[i]);
    std::array<UnityGraphicsD3D12ResourceState, 4> states{};
    int count = 0;
    for (size_t i = 0; i < 4; ++i) if (resources[i]) {
        auto state = hudless ? D3D12_RESOURCE_STATE_RENDER_TARGET : i == 3 ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        states[static_cast<size_t>(count++)] = {resources[i].Get(), state, state};
    }
    slot.leases = std::move(resources);
    const auto complete = unity->ExecuteCommandList(slot.list.Get(), count, states.data());
    // Missing completion proof stops the bridge and keeps the command allocator owned.
    const auto source = source_of(pass);
    if (!rsf_unity_frames_submitted(&admission, complete, &source))
        log("Unity D3D12 submission could not be tracked; adapter stopped and remains owned.");
#else
    (void)entry;
#endif
}
catch (...) {
    rsf_unity_frame_callback call;
    if (call.entered && ++refusals <= 3) log("Unity render callback refused; spatial fallback remains queued.");
}
}

namespace {

std::atomic<uint64_t> input_source{};
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
    rsf_unity_frame_callback call;
    if (call.entered && code>=0 && removed==PM_REMOVE) {
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
    rsf_unity_frame_callback call;
    if (!call.entered || !active() || !fact || fact->struct_size<sizeof(*fact) || fact->session_id!=services.session_id || !services.cpu_event) return;
    auto copy=*fact;
    if (copy.stage==RSF_GAME_CPU_INPUT_EVENT) {
        copy.source_frame_id=input_source.load();
        if (!copy.source_frame_id) copy.flags|=RSF_GAME_CPU_INPUT_UNFRAMED;
    }
    services.cpu_event(services.user,&copy);
}
void cpu_event(uint32_t stage, uint64_t frame_id) {
    rsf_unity_frame_callback call;
    if (!call.entered || !active() || !frame_id || !services.cpu_event) return;
    LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    if (stage==RSF_GAME_CPU_FRAME_BEGIN) {
        input_source.store(frame_id);
        if (!message_hook) message_hook=SetWindowsHookExW(WH_GETMESSAGE,received_message,nullptr,GetCurrentThreadId());
    }
    rsf_unity_frames_cpu_stage(&services, stage, frame_id, uint64_t(now.QuadPart), uint64_t(frequency.QuadPart));
    if (stage==RSF_GAME_CPU_FRAME_END) input_source.store(0);
}
}
bool rsf_unity_bridge_prepare(const rsf_game_host_services& host, rsf_unity_native_api* api) noexcept
{
    if (!api || !host.render_pass || !host.render_config || !host.log) return false;
    std::lock_guard<std::mutex> lock(guard);
    services = host;
    *api = {sizeof(*api), RSF_UNITY_NATIVE_ABI_VERSION, host.session_id,
        [](const char* text) { rsf_unity_frame_callback call; if (call.entered) log(text); }, config, enqueue,
        reinterpret_cast<void*>(render), rsf_unity_frames_report_managed, cpu_event, cpu_fact};
    return true;
}
void rsf_unity_bridge_activate(bool enabled) noexcept { rsf_unity_frames_activate(enabled ? 1u : 0u); }
#if defined(RSF_UNITY_PACKAGE_BRIDGE)
bool rsf_unity_d3d12_try_close() noexcept
{
    std::lock_guard<std::mutex> lock(guard);
    if (!rsf_unity_frames_try_close()) return false;
    const auto value = completed();
    return rsf_unity_frames_drained(value) != 0;
}
#else
bool rsf_unity_bridge_drained() noexcept
{
    std::lock_guard<std::mutex> lock(guard);
    return rsf_unity_frames_drained(completed()) != 0;
}
#endif
void rsf_unity_bridge_release() noexcept
{
    std::lock_guard<std::mutex> lock(guard);
    if (message_hook) {UnhookWindowsHookEx(message_hook);message_hook=nullptr;} input_source.store(0);
    commands = {}; fence.Reset(); services = {};
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
