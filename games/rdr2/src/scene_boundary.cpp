// SPDX-License-Identifier: GPL-3.0-only
#include "scene_boundary.h"
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <MinHook.h>
#include <intrin.h>
#include <array>
#include <atomic>
#include <cstring>
#include <mutex>

namespace {
using Microsoft::WRL::ComPtr;
using Blit = uintptr_t (*)(void*, uint32_t, uint32_t, const void*, const void*, const void*);
using Marker = bool (*)(uint32_t, uint64_t);
using Output = uintptr_t (*)(void*, void*, uint32_t);
using Sleep = bool (*)();
using Mode = int32_t (*)(void*, const void*);
Blit original_blit{};
Marker original_marker{};
Output original_output{};
Sleep original_sleep{};
Mode original_mode{};
thread_local bool output_scope{};
const unsigned char* image{};
rsf_game_host_services host{};
ComPtr<IDXGISwapChain3> swapchain;
std::mutex guard;
std::atomic<bool> active{};
std::atomic<uint32_t> readers{};
bool prepared{};
struct Scope { Scope() { ++readers; } ~Scope() { --readers; } };
struct Markers { uint64_t frame{}; uint32_t mask{}; };
struct Scene {
    rsf_game_render_pass pass{};
    void* commands{};
    uintptr_t surface{};
    uint32_t ordinal{};
};
std::array<Markers, 128> markers;
std::array<Scene, 64> scenes;
bool read(uintptr_t address, void* destination, size_t bytes)
{
    SIZE_T copied{};
    return address && ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<void*>(address), destination,
        bytes, &copied) && copied == bytes;
}
template<class T> bool at(uintptr_t address, T& value) { return read(address, &value, sizeof(value)); }
void log(const char* message) { if (host.log) host.log(host.user, message); }
bool latency(uint32_t kind, uint64_t frame)
{
    if (!active.load() || !host.process_latency) return false;
    LARGE_INTEGER now{}, frequency{};
    QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    const rsf_game_latency_event event{sizeof(event), kind, host.session_id, frame,
        static_cast<uint64_t>(now.QuadPart), static_cast<uint64_t>(frequency.QuadPart)};
    return host.process_latency(host.user, &event) == 0;
}
bool sleep()
{
    Scope scope;
    uint32_t current{};
    // Same producer as the game's INPUT_SAMPLE caller at abcfea: direct counter + 1.
    if (at(reinterpret_cast<uintptr_t>(image) + 0x542904c, current) &&
        latency(RSF_GAME_LATENCY_SLEEP, static_cast<uint32_t>(current + 1u))) return true;
    return original_sleep();
}
int32_t mode(void* device, const void* parameters)
{
    Scope scope;
    return latency(RSF_GAME_LATENCY_MODE, 0) ? 0 : original_mode(device, parameters);
}
bool context(uintptr_t& draw, uint64_t& frame)
{
    uint32_t index{}, flags{}, counter{};
    uintptr_t block{};
    const auto base = reinterpret_cast<uintptr_t>(image);
    if (!at(base + 0x5b403e0, index) || index >= 4096 ||
        !at(__readgsqword(0x58) + index * sizeof(void*), block) || !at(block + 0x14f0, draw) ||
        !at(block + 0x61c, flags)) return false;
    if (flags & 6) {
        uintptr_t pointer{};
        if (!at(base + 0x5429058, pointer) || !at(pointer, counter)) return false;
    } else if (!at(base + 0x542904c, counter)) return false;
    frame = counter;
    return draw && frame;
}
void forget_resources(rsf_game_render_pass& pass)
{
    pass.color_input = pass.color_output = pass.depth = pass.motion = pass.exposure = nullptr;
    pass.color_output_readable = pass.ui_input = pass.swapchain = pass.scene_surface = pass.sampled_texture = nullptr;
}
uintptr_t output(void* depth, void* color, uint32_t technique)
{
    Scope scope;
    const auto* caller = static_cast<const unsigned char*>(_ReturnAddress());
    const bool selected = caller == image + 0x5cab01 || caller == image + 0x5cad9b || caller == image + 0x5cadf1;
    const bool previous = output_scope;
    output_scope = active.load() && selected;
    const auto result = original_output(depth, color, technique);
    output_scope = previous;
    return result;
}
uintptr_t blit(void* shader, uint32_t technique, uint32_t pass_index,
    const void* source_rect, const void* destination_rect, const void* clip_rect)
{
    Scope scope;
    const bool selected = output_scope && _ReturnAddress() == image + 0x5c11b9;
    const auto result = original_blit(shader, technique, pass_index, source_rect, destination_rect, clip_rect);
    if (!active.load() || !selected || !host.render_pass) return result;
    uintptr_t draw{}, commands{}, view{}, texture{}, resource{};
    uint64_t frame{};
    uint32_t count{}, slice{};
    // 26d4964 writes the colour-view array/count; 26ddf80 reads view+8 as its texture.
    // The live DX12 renderer's vtable+248 is 26b4250: mov rax,[rdx+48]; ret.
    if (!context(draw, frame) || !at(draw + 0xcc30, commands) || !at(draw + 0xca94, count) || count != 1 ||
        !at(draw + 0xca48, view) || !at(view + 0x14, slice) || ((slice >> 4) & 15u) ||
        !at(view + 8, texture) || !at(texture + 0x48, resource)) return result;
    ComPtr<IDXGISwapChain3> chain;
    rsf_game_render_pass pass{};
    {
        std::lock_guard lock(guard);
        auto& saved = scenes[frame % scenes.size()];
        if (!swapchain || saved.pass.native_frame != frame || saved.commands != reinterpret_cast<void*>(commands)) return result;
        chain = swapchain; pass = saved.pass;
    }
    // Match the actual colour resource, rather than the swapchain's current CPU index on a
    // recording worker. References are local to this callback and cannot block later resize.
    DXGI_SWAP_CHAIN_DESC description{};
    if (FAILED(chain->GetDesc(&description)) || description.BufferCount > 8) return result;
    ComPtr<ID3D12Resource> surface;
    for (UINT i = 0; i < description.BufferCount; ++i) {
        ComPtr<ID3D12Resource> candidate;
        if (SUCCEEDED(chain->GetBuffer(i, IID_PPV_ARGS(&candidate))) &&
            reinterpret_cast<uintptr_t>(candidate.Get()) == resource) { surface = candidate; break; }
    }
    if (!surface) return result;
    const auto dimensions = surface->GetDesc();
    if (dimensions.Width != pass.camera.output_width || dimensions.Height != pass.camera.output_height ||
        dimensions.SampleDesc.Count != 1) return result;
    {
        std::lock_guard lock(guard);
        auto& saved = scenes[frame % scenes.size()];
        if (saved.pass.native_frame != frame || saved.commands != reinterpret_cast<void*>(commands)) return result;
        if (saved.ordinal >= 16) return result;
        pass.pass_key = ++saved.ordinal;
        saved.surface = resource;
        pass.source_frame_id = saved.pass.source_frame_id;
    }
    pass.role = RSF_GAME_RENDER_FINAL_SCENE;
    pass.color_input = pass.scene_surface = surface.Get(); pass.swapchain = chain.Get();
    pass.window_key = reinterpret_cast<uint64_t>(description.OutputWindow);
    host.render_pass(host.user, reinterpret_cast<void*>(commands), &pass, 0);
    return result;
}
bool marker(uint32_t kind, uint64_t frame)
{
    Scope scope;
    const bool handled = kind <= 6 && latency(kind, frame);
    rsf_game_render_pass window{};
    ComPtr<IDXGISwapChain3> chain;
    if (active.load() && frame && kind <= 6) {
        std::lock_guard lock(guard);
        auto& state = markers[frame % markers.size()];
        if (state.frame != frame) state = {frame, 0};
        state.mask |= 1u << kind;
        if (kind == 4 && host.render_pass && swapchain) {
            const auto& scene = scenes[frame % scenes.size()];
            if (scene.pass.native_frame == frame && scene.surface) {
                window = scene.pass;
                window.role = RSF_GAME_RENDER_WINDOW;
                chain = swapchain;
            }
        }
    }
    if (chain) {
        ComPtr<ID3D12Resource> presented;
        if (SUCCEEDED(chain->GetBuffer(chain->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&presented)))) {
            window.swapchain = chain.Get(); window.scene_surface = presented.Get();
            host.render_pass(host.user, nullptr, &window, 1);
        }
    }
    return handled ? true : original_marker(kind, frame);
}
}
bool rsf_rdr2_scene_prepare(const void* module, const rsf_game_host_services& services) noexcept
{
    if (prepared || !module || !services.render_pass) return false;
    image = static_cast<const unsigned char*>(module); host = services;
    constexpr unsigned char blit_bytes[] = {0x4c,0x8b,0xdc,0x49,0x89,0x5b,0x08,0x49,0x89,0x6b,0x10,0x49,0x89,0x73,0x18,0x49};
    constexpr unsigned char marker_bytes[] = {0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x54,0x24,0x10,0x55,0x48,0x8d,0x6c,0x24,0xa9};
    constexpr unsigned char native_texture[] = {0x48,0x8b,0x42,0x48,0xc3};
    constexpr unsigned char output_bytes[] = {0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x74,0x24,0x18,0x57,0x48,0x83,0xec,0x50,0x41};
    constexpr unsigned char sleep_bytes[] = {0x40,0x53,0x48,0x83,0xec,0x50,0x80,0x3d,0x06,0x3d,0x09,0x03,0x00,0x0f,0x84,0xc4};
    constexpr unsigned char mode_bytes[] = {0x48,0x89,0x74,0x24,0x10,0x57,0x48,0x83,0xec,0x20,0x48,0x8b,0xfa,0x48,0x8b,0xf1};
    if (std::memcmp(image + 0x5c1254, blit_bytes, sizeof(blit_bytes)) ||
        std::memcmp(image + 0x26d4674, marker_bytes, sizeof(marker_bytes)) ||
        std::memcmp(image + 0x26b4250, native_texture, sizeof(native_texture)) ||
        std::memcmp(image + 0x5f1e08, output_bytes, sizeof(output_bytes)) ||
        std::memcmp(image + 0x2687fb4, sleep_bytes, sizeof(sleep_bytes)) ||
        std::memcmp(image + 0x1e800, mode_bytes, sizeof(mode_bytes))) {
        log("RDR2 scene producer refused: native blit/marker/texture bytes differ."); return false;
    }
    for (const auto site : {0x5caafc, 0x5cad96, 0x5cadec}) {
        int32_t displacement{};
        std::memcpy(&displacement, image + site + 1, sizeof(displacement));
        if (image[site] != 0xe8 || site + 5 + displacement != 0x5f1e08) {
            log("RDR2 scene producer refused: postfx output callers differ."); return false;
        }
    }
    if (MH_CreateHook(const_cast<unsigned char*>(image + 0x5f1e08), reinterpret_cast<void*>(&output),
        reinterpret_cast<void**>(&original_output)) != MH_OK) return false;
    if (MH_CreateHook(const_cast<unsigned char*>(image + 0x5c1254), reinterpret_cast<void*>(&blit),
        reinterpret_cast<void**>(&original_blit)) != MH_OK) {
        MH_RemoveHook(const_cast<unsigned char*>(image + 0x5f1e08)); return false;
    }
    if (MH_CreateHook(const_cast<unsigned char*>(image + 0x26d4674), reinterpret_cast<void*>(&marker),
        reinterpret_cast<void**>(&original_marker)) != MH_OK) {
        MH_RemoveHook(const_cast<unsigned char*>(image + 0x5c1254));
        MH_RemoveHook(const_cast<unsigned char*>(image + 0x5f1e08)); return false;
    }
    if (MH_CreateHook(const_cast<unsigned char*>(image + 0x2687fb4), reinterpret_cast<void*>(&sleep),
        reinterpret_cast<void**>(&original_sleep)) != MH_OK) {
        MH_RemoveHook(const_cast<unsigned char*>(image + 0x26d4674));
        MH_RemoveHook(const_cast<unsigned char*>(image + 0x5c1254));
        MH_RemoveHook(const_cast<unsigned char*>(image + 0x5f1e08)); return false;
    }
    if (MH_CreateHook(const_cast<unsigned char*>(image + 0x1e800), reinterpret_cast<void*>(&mode),
        reinterpret_cast<void**>(&original_mode)) != MH_OK) {
        MH_RemoveHook(const_cast<unsigned char*>(image + 0x2687fb4));
        MH_RemoveHook(const_cast<unsigned char*>(image + 0x26d4674));
        MH_RemoveHook(const_cast<unsigned char*>(image + 0x5c1254));
        MH_RemoveHook(const_cast<unsigned char*>(image + 0x5f1e08)); return false;
    }
    prepared = true;
    log("RDR2 scene producer prepared at +5c1254; native frame markers observed at +26d4674.");
    return true;
}
bool rsf_rdr2_scene_start() noexcept
{
    if (!prepared) return false;
    active.store(true);
    if (MH_EnableHook(const_cast<unsigned char*>(image + 0x5f1e08)) == MH_OK &&
        MH_EnableHook(const_cast<unsigned char*>(image + 0x26d4674)) == MH_OK &&
        MH_EnableHook(const_cast<unsigned char*>(image + 0x2687fb4)) == MH_OK &&
        MH_EnableHook(const_cast<unsigned char*>(image + 0x1e800)) == MH_OK &&
        MH_EnableHook(const_cast<unsigned char*>(image + 0x5c1254)) == MH_OK) return true;
    active.store(false);
    MH_DisableHook(const_cast<unsigned char*>(image + 0x26d4674));
    MH_DisableHook(const_cast<unsigned char*>(image + 0x5f1e08));
    MH_DisableHook(const_cast<unsigned char*>(image + 0x2687fb4));
    MH_DisableHook(const_cast<unsigned char*>(image + 0x1e800));
    return false;
}
bool rsf_rdr2_scene_quiesce() noexcept { active.store(false); return readers.load() == 0; }
void rsf_rdr2_scene_stop() noexcept
{
    if (!prepared || active.load()) return;
    MH_DisableHook(const_cast<unsigned char*>(image + 0x5c1254));
    MH_DisableHook(const_cast<unsigned char*>(image + 0x26d4674));
    MH_DisableHook(const_cast<unsigned char*>(image + 0x5f1e08));
    MH_DisableHook(const_cast<unsigned char*>(image + 0x2687fb4));
    MH_DisableHook(const_cast<unsigned char*>(image + 0x1e800));
    if (readers.load()) return;
    MH_RemoveHook(const_cast<unsigned char*>(image + 0x5c1254));
    MH_RemoveHook(const_cast<unsigned char*>(image + 0x26d4674));
    MH_RemoveHook(const_cast<unsigned char*>(image + 0x5f1e08));
    MH_RemoveHook(const_cast<unsigned char*>(image + 0x2687fb4));
    MH_RemoveHook(const_cast<unsigned char*>(image + 0x1e800));
    prepared = false;
}
void rsf_rdr2_scene_upscale(void* commands, const rsf_game_render_pass& pass) noexcept
{
    if (!prepared || !active.load()) return;
    std::lock_guard lock(guard);
    auto& scene = scenes[pass.native_frame % scenes.size()];
    if (scene.pass.native_frame == pass.native_frame) {
        // Multiple successful upscale views cannot silently share one primary scene identity.
        scene.commands = nullptr; scene.pass.source_frame_id = 0; scene.pass.camera_valid = 0; return;
    }
    scene = {}; scene.pass = pass; scene.commands = commands; forget_resources(scene.pass);
    scene.pass.source_frame_id = pass.source_frame_id;
}
void rsf_rdr2_scene_swapchain(void* pointer) noexcept
{
    ComPtr<IDXGISwapChain3> chain;
    if (!pointer || FAILED(static_cast<IUnknown*>(pointer)->QueryInterface(IID_PPV_ARGS(&chain)))) return;
    std::lock_guard lock(guard); swapchain = chain;
}
uint64_t rsf_rdr2_scene_source(uint64_t frame) noexcept
{
    if (!active.load() || !frame) return 0;
    std::lock_guard lock(guard);
    const auto& state = markers[frame % markers.size()];
    constexpr uint32_t required = (1u << 0) | (1u << 1) | (1u << 2) | (1u << 6);
    return state.frame == frame && (state.mask & required) == required ? frame : 0;
}
