// SPDX-License-Identifier: MIT
// Unity SDK registration and Win32 input observations for the shared D3D12 bridge.
#include "bridge.h"
#include "bridge_d3d12.h"
#include "../../../integrations/windows/window_input.h"
#include <rescaleframe/sha256_file.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <array>
#include <atomic>
#include <cstring>
#include <mutex>
#if RSF_HAVE_UNITY_NATIVE
#include <IUnityGraphics.h>
#include <IUnityGraphicsD3D12.h>
#endif
#if defined(RSF_UNITY_PACKAGE_BRIDGE)
extern "C" void rsf_unity_package_sr_slot_reset(uint32_t bridge, void* commands, uint32_t slot);
#endif

namespace {
// Orders host preparation and shutdown with packet publication.
std::mutex guard;
rsf_game_host_services services{};
constexpr rsf_unity_texture_ops texture_ops{rsf_unity_retain_texture, rsf_unity_release_texture};
std::atomic<uint32_t> refusals{0};
int event_id = 0;
bool active() { return rsf_unity_frames_active() != 0; }
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
    if (!rsf_common_sha256_file(path, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, digest)) return false;
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
// Unity's versioned C++ interface and its resource-state record stay at the SDK boundary.
static_assert(sizeof(rsf_unity_d3d12_engine) == 8 * sizeof(void*));
constexpr rsf_unity_d3d12_engine engine{
    []() -> void* { return unity ? unity->GetDevice() : nullptr; },
    []() -> void* { return unity->GetCommandQueue(); },
    []() -> void* { return unity->GetSwapChain(); },
    []() -> void* { return unity->GetFrameFence(); },
    []() -> uint64_t { return unity->GetNextFrameFenceValue(); },
    []() -> void* {
        UnityGraphicsD3D12RecordingState recording{};
        return unity->CommandRecordingState(&recording) ? recording.commandList : nullptr;
    },
    [](void* list, uint32_t count, void* const* resources, const int32_t* states) -> uint64_t {
        std::array<UnityGraphicsD3D12ResourceState, 4> native{};
        for (uint32_t i = 0; i < count; ++i) {
            const auto state = static_cast<D3D12_RESOURCE_STATES>(states[i]);
            native[i] = {static_cast<ID3D12Resource*>(resources[i]), state, state};
        }
        return unity->ExecuteCommandList(static_cast<ID3D12GraphicsCommandList*>(list),
            static_cast<int>(count), native.data());
    },
#if defined(RSF_UNITY_PACKAGE_BRIDGE)
    rsf_unity_package_sr_slot_reset,
#else
    nullptr,
#endif
};
#endif

void log(const char* message)
{
    if (services.log) services.log(services.user, message);
}
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
    rsf_unity_d3d12_render_packet(&engine, &services, static_cast<uint32_t>(id - event_id), &entry.packet);
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
        const auto& message=*reinterpret_cast<const MSG*>(argument);
        received_input(rsf_win32_input_kind(message.message,message.lParam),message.message);
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
#ifndef RSF_UNITY_PACKAGE_BRIDGE
    rsf_unity_frames_open_callbacks();
#endif
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
    const auto value = rsf_unity_d3d12_completed();
    return rsf_unity_frames_drained(value) != 0;
}
#else
extern "C" uint32_t rsf_unity_mono_prepare(const rsf_game_host_services* host, rsf_unity_native_api* api) noexcept
{
    return host && rsf_unity_bridge_prepare(*host, api);
}

extern "C" uint32_t rsf_unity_mono_try_close() noexcept
{
    std::lock_guard<std::mutex> lock(guard);
    return rsf_unity_frames_try_close_completed(rsf_unity_d3d12_completed()) != 0;
}
#endif
void rsf_unity_bridge_release() noexcept
{
    std::lock_guard<std::mutex> lock(guard);
    if (message_hook) {UnhookWindowsHookEx(message_hook);message_hook=nullptr;} input_source.store(0);
    rsf_unity_d3d12_release_graphics(); services = {};
    rsf_unity_frames_release();
}
#ifndef RSF_UNITY_PACKAGE_BRIDGE
extern "C" void rsf_unity_mono_release() noexcept { rsf_unity_bridge_release(); }
#endif
uint32_t rsf_unity_bridge_managed_state() noexcept { return rsf_unity_frames_managed_stage(); }
extern "C" __declspec(dllexport) void* rsf_unity_get_render_event() noexcept
{
#ifndef RSF_UNITY_PACKAGE_BRIDGE
    rsf_unity_frame_callback call;
    if (!call.entered) return nullptr;
#endif
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
