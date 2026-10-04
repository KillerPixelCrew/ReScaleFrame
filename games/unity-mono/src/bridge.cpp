// SPDX-License-Identifier: GPL-3.0-only
#include "bridge.h"
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <array>
#include <atomic>
#include <cmath>
#include <cstring>
#include <mutex>
#include <bcrypt.h>
#include <vector>
#if RSF_HAVE_UNITY_NATIVE
#include <IUnityGraphics.h>
#include <IUnityGraphicsD3D12.h>
#endif

namespace {
using Microsoft::WRL::ComPtr;
struct Pending {
    rsf_unity_packet packet{};
    std::array<ComPtr<ID3D12Resource>, 4> resources;
    bool used = false;
};
struct Commands {
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;
    std::array<ComPtr<ID3D12Resource>, 4> leases;
    uint64_t complete = 0;
};
std::mutex guard;
rsf_game_host_services services{};
std::array<Pending, 32> pending;
std::array<Commands, 3> commands;
std::array<Commands, 3> hud_commands;
ComPtr<ID3D12Fence> fence;
std::atomic<bool> active{false};
std::atomic<uint32_t> callbacks{0};
std::atomic<uint32_t> managed_stage{0};
uint32_t next_commands = 0;
uint32_t next_hud_commands = 0;
std::atomic<uint32_t> backend{1}, quality{1}, generation{1};
std::atomic<bool> policy_pending{false};
uint32_t refusals = 0;
uint32_t output_width = 0, output_height = 0;
uint32_t input_width = 0, input_height = 0;
bool submission_unknown = false;
int event_id = 0;
void report_state(uint32_t stage) { managed_stage.store(stage); }
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
    HANDLE file = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_DELETE, nullptr, OPEN_EXISTING, 0, nullptr);
    if (file == INVALID_HANDLE_VALUE) return false;
    BCRYPT_ALG_HANDLE algorithm = nullptr; BCRYPT_HASH_HANDLE hash = nullptr;
    bool ok = BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) >= 0;
    ok = ok && BCryptCreateHash(algorithm, &hash, nullptr, 0, nullptr, 0, 0) >= 0;
    std::vector<unsigned char> buffer(65536); DWORD read = 0;
    while (ok) { ok = ReadFile(file, buffer.data(), static_cast<DWORD>(buffer.size()), &read, nullptr) != 0;
        if (!ok || !read) break; ok = BCryptHashData(hash, buffer.data(), read, 0) >= 0; }
    unsigned char digest[32]{}; ok = ok && BCryptFinishHash(hash, digest, sizeof(digest), 0) >= 0;
    if (hash) BCryptDestroyHash(hash); if (algorithm) BCryptCloseAlgorithmProvider(algorithm, 0); CloseHandle(file);
    constexpr unsigned char expected[] = {0xbb,0xdf,0x3e,0x73,0x52,0x22,0x23,0xea,0x45,0xc3,0xa5,0x40,0x49,0xb7,0x16,0xf9,0xda,0x3e,0x63,0x29,0x94,0x63,0x67,0x34,0xc5,0xd5,0x33,0x26,0xaa,0x2c,0x36,0xd0};
    return ok && !std::memcmp(digest, expected, 32);
}
void configure_graphics()
{
    if (!graphics || !unity || graphics->GetRenderer() != kUnityGfxRendererD3D12) { unity = nullptr; return; }
    event_id = graphics->ReserveEventIDRange(2);
    UnityD3D12PluginEventConfig event{};
    event.graphicsQueueAccess = kUnityD3D12GraphicsQueueAccess_DontCare;
    event.flags = kUnityD3D12EventConfigFlag_SyncWorkerThreads | kUnityD3D12EventConfigFlag_FlushCommandBuffers;
    unity->ConfigureEvent(event_id, &event);
    event.graphicsQueueAccess = kUnityD3D12GraphicsQueueAccess_Allow;
    unity->ConfigureEvent(event_id + 1, &event);
}
#endif

void log(const char* message)
{
    if (services.log) services.log(services.user, message);
}
int config(uint32_t width, uint32_t height, rsf_unity_config* output)
{
    if (!output || output->struct_size != sizeof(*output) || output->abi_version != RSF_UNITY_BRIDGE_ABI_VERSION) return 0;
    rsf_game_render_config host{}; host.struct_size = sizeof(host);
    if (!services.render_config || !services.render_config(services.user, &host)) return 0;
    *output = {sizeof(*output), RSF_UNITY_BRIDGE_ABI_VERSION, active.load() ? host.enabled : 0u,
        backend.load(), quality.load(), generation.load(), host.render_width, host.render_height, width, height};
    if (policy_pending.load()) output->enabled = 0;
    if (host.output_width != width || host.output_height != height) output->enabled = 0;
    return 1;
}
bool valid(const rsf_unity_packet& packet)
{
    const auto& c = packet.camera;
    if (packet.struct_size != sizeof(packet) || packet.abi_version != RSF_UNITY_BRIDGE_ABI_VERSION ||
        packet.session_id != services.session_id || !packet.frame_id || !packet.view_key || (!(packet.flags & 12) && !packet.color) ||
        c.struct_size != sizeof(c) || c.abi_version != RSF_GAME_FRAME_ABI_VERSION ||
        !c.render_width || !c.render_height || !c.output_width || !c.output_height ||
        c.render_width > 16384 || c.render_height > 16384 || c.output_width > 16384 || c.output_height > 16384 ||
        !std::isfinite(c.near_plane) || !std::isfinite(c.far_plane) || c.near_plane <= 0 || c.far_plane <= c.near_plane)
        return false;
    if (!(packet.flags & 6) && (!packet.depth || !packet.motion || !packet.output)) return false;
    if ((packet.flags & 32) && (!packet.depth || !packet.motion)) return false;
    for (float value : c.view_to_clip) if (!std::isfinite(value)) return false;
    for (float value : c.clip_to_view) if (!std::isfinite(value)) return false;
    for (float value : c.clip_to_previous_clip) if (!std::isfinite(value)) return false;
    return std::isfinite(c.jitter_pixels[0]) && std::isfinite(c.jitter_pixels[1]);
}
void* enqueue(const rsf_unity_packet* packet)
{
    if (!active.load() || !packet) return nullptr;
    std::lock_guard<std::mutex> lock(guard);
    if (!valid(*packet)) return nullptr;
    for (auto& entry : pending) if (!entry.used) {
        entry.packet = *packet;
        void* textures[] = {packet->color, packet->depth, packet->motion, packet->output};
        for (size_t i = 0; i < 4; ++i) entry.resources[i] = static_cast<ID3D12Resource*>(textures[i]);
        entry.used = true;
        return &entry.packet;
    }
    return nullptr;
}
[[maybe_unused]] void make_pass(const rsf_unity_packet& packet, rsf_game_render_pass& pass)
{
    pass = {}; pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_SR;
    pass.session_id = packet.session_id; pass.family_key = pass.view_key = packet.view_key;
    pass.history_key = packet.view_key; pass.native_frame = packet.frame_id; pass.resource_generation = packet.generation;
    pass.source_frame_id = packet.frame_id;
    pass.flags = packet.flags & 1; pass.color_input = packet.color; pass.color_output = packet.output;
    pass.depth = packet.depth; pass.motion = packet.motion; pass.camera = packet.camera;
    pass.camera_valid = (packet.flags & 2) && !(packet.flags & 32) ? 0u : 1u;
    if (packet.flags & 32) pass.role = RSF_GAME_RENDER_FG_INPUTS;
    pass.render_rect[2] = static_cast<int32_t>(packet.camera.render_width);
    pass.render_rect[3] = static_cast<int32_t>(packet.camera.render_height);
    pass.output_rect[2] = static_cast<int32_t>(packet.camera.output_width);
    pass.output_rect[3] = static_cast<int32_t>(packet.camera.output_height);
    pass.motion_to_uv[0] = pass.motion_to_uv[1] = 1; pass.motion_camera_included = 1;
    std::memcpy(pass.previous_clip_to_clip, packet.previous_clip_to_clip, sizeof(pass.previous_clip_to_clip));
}
void __stdcall render(int id, void* address) noexcept try
{
    if (id != event_id && id != event_id + 1) return;
    callbacks.fetch_add(1);
    struct Finish { ~Finish() { callbacks.fetch_sub(1); } } finish;
    Pending entry;
    {
        std::lock_guard<std::mutex> lock(guard);
        for (auto& candidate : pending) if (&candidate.packet == address && candidate.used) {
            entry = std::move(candidate); candidate = {}; break;
        }
    }
    if (!entry.used) return;
#if RSF_HAVE_UNITY_NATIVE
    if (!active.load() || !unity || !services.render_pass) return;
    ID3D12Device* device = unity->GetDevice();
    if (!device) return;
    if (entry.packet.flags & 8) {
        ComPtr<IDXGISwapChain3> chain;
        auto* owned = unity->GetSwapChain();
        if (!owned || FAILED(owned->QueryInterface(IID_PPV_ARGS(&chain))) ||
            FAILED(chain->GetBuffer(chain->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&entry.resources[0])))) return;
        entry.packet.color = entry.resources[0].Get();
    }
    if (id == event_id + 1 && (entry.packet.flags & 4)) {
        rsf_game_render_pass pass{}; make_pass(entry.packet, pass);
        pass.role = RSF_GAME_RENDER_WINDOW; pass.swapchain = unity->GetSwapChain();
        services.render_pass(services.user, unity->GetCommandQueue(), &pass, 1);
        return;
    }
    if (!fence) fence = unity->GetFrameFence();
    if (!fence) return;
    const bool hudless = (entry.packet.flags & 8) != 0;
    if (!hudless && (input_width != entry.packet.camera.render_width || input_height != entry.packet.camera.render_height)) {
        for (const auto& previous : commands)
            if (previous.complete && fence->GetCompletedValue() < previous.complete) return;
        input_width = entry.packet.camera.render_width; input_height = entry.packet.camera.render_height;
    }
    const bool changing = policy_pending.load();
    if (changing) {
        for (const auto& previous : commands)
            if (previous.complete && fence->GetCompletedValue() < previous.complete) return;
        for (const auto& previous : hud_commands)
            if (previous.complete && fence->GetCompletedValue() < previous.complete) return;
        policy_pending.store(false);
    } else if (entry.packet.generation != generation.load()) return;
    if ((output_width != entry.packet.camera.output_width || output_height != entry.packet.camera.output_height)) {
        for (const auto& previous : commands)
            if (previous.complete && fence->GetCompletedValue() < previous.complete) return;
        output_width = entry.packet.camera.output_width; output_height = entry.packet.camera.output_height;
    }
    auto& slot = hudless ? hud_commands[next_hud_commands] : commands[next_commands];
    // Reuse only after Unity's completion fence. Busy slots leave the queued spatial fallback.
    if (slot.complete && fence->GetCompletedValue() < slot.complete) return;
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
        active.store(false); log("Unity D3D12 device changed; adapter requires a drained restart."); return;
    }
    if (FAILED(slot.allocator->Reset()) || FAILED(slot.list->Reset(slot.allocator.Get(), nullptr))) return;
    rsf_game_render_pass pass{}; make_pass(entry.packet, pass);
    if (hudless) pass.role = RSF_GAME_RENDER_FINAL_SCENE;
    if (changing) { pass.flags |= 0x80000000u; pass.camera_valid = 0; }
    services.render_pass(services.user, slot.list.Get(), &pass, 1);
    if (FAILED(slot.list->Close())) return;
    std::array<UnityGraphicsD3D12ResourceState, 4> states{};
    int count = 0;
    for (size_t i = 0; i < 4; ++i) if (entry.resources[i]) {
        auto state = hudless ? D3D12_RESOURCE_STATE_RENDER_TARGET : i == 3 ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        states[static_cast<size_t>(count++)] = {entry.resources[i].Get(), state, state};
    }
    slot.leases = std::move(entry.resources);
    slot.complete = unity->ExecuteCommandList(slot.list.Get(), count, states.data());
    // A zero completion identity is an ownership failure, not permission to reuse the allocator.
    if (!slot.complete) { submission_unknown = true; active.store(false); log("Unity D3D12 submission returned no fence; adapter stopped and remains owned."); return; }
    services.render_pass(services.user, slot.list.Get(), &pass, 0);
    if (hudless) next_hud_commands = (next_hud_commands + 1) % static_cast<uint32_t>(hud_commands.size());
    else next_commands = (next_commands + 1) % static_cast<uint32_t>(commands.size());
#else
    (void)entry;
#endif
}
catch (...) { if (++refusals <= 3) log("Unity render callback refused; spatial fallback remains queued."); }
}

namespace {
void cpu_event(uint32_t stage, uint64_t frame_id) {
    if (!active.load() || !frame_id || !services.cpu_event) return;
    LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    rsf_game_cpu_event event{}; event.struct_size = sizeof(event); event.stage = stage;
    event.session_id = services.session_id; event.source_frame_id = frame_id;
    event.timestamp_qpc = uint64_t(now.QuadPart); event.qpc_frequency = uint64_t(frequency.QuadPart);
    services.cpu_event(services.user, &event);
}
}
bool rsf_unity_bridge_prepare(const rsf_game_host_services& host, rsf_unity_native_api* api) noexcept
{
    if (!api || !host.render_pass || !host.render_config || !host.log) return false;
    std::lock_guard<std::mutex> lock(guard);
    services = host;
    *api = {sizeof(*api), RSF_UNITY_NATIVE_ABI_VERSION, host.session_id, log, config, enqueue,
        reinterpret_cast<void*>(render), report_state, cpu_event};
    return true;
}
void rsf_unity_bridge_activate(bool enabled) noexcept { active.store(enabled); }
bool rsf_unity_bridge_drained() noexcept
{
    std::lock_guard<std::mutex> lock(guard);
    if (callbacks.load() || submission_unknown) return false;
    for (const auto& entry : pending) if (entry.used) return false;
    for (const auto& slot : commands) if (slot.complete && (!fence || fence->GetCompletedValue() < slot.complete)) return false;
    for (const auto& slot : hud_commands) if (slot.complete && (!fence || fence->GetCompletedValue() < slot.complete)) return false;
    return true;
}
void rsf_unity_bridge_release() noexcept
{
    std::lock_guard<std::mutex> lock(guard);
    commands = {}; hud_commands = {}; pending = {}; fence.Reset(); services = {}; next_commands = next_hud_commands = 0;
    output_width = output_height = 0;
    input_width = input_height = 0;
    submission_unknown = false;
    managed_stage.store(0);
}
uint32_t rsf_unity_bridge_managed_state() noexcept { return managed_stage.load(); }
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
extern "C" __declspec(dllexport) int rsf_unity_set_policy(uint32_t requested_backend, uint32_t requested_quality) noexcept
{
    if (requested_backend > 6 || requested_quality > 5) return 0;
    backend.store(requested_backend); quality.store(requested_quality);
    generation.fetch_add(1);
    if (active.load()) policy_pending.store(true);
    return 1;
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
    active.store(false);
    // Callback/device retirement is coordinated by the lifecycle before explicit unload.
    unity = nullptr; graphics = nullptr;
}
#endif
