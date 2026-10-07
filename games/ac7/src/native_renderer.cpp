// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/ac7_native_renderer.h>
#include <rescaleframe/ac7_view.h>
#include "truesky_depth.h"
#include "truesky_motion.h"
#include "contact_shadow.h"
#include <d3d11.h>
#include <wrl/client.h>
#include <algorithm>
#include <array>
#include <vector>
#include <windows.h>
#include <MinHook.h>
#include <atomic>
#include <cstring>
#include <cmath>
#include <cstdio>
#include <mutex>
#include <new>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

// Installs AC7 renderer hooks only after validating each target against its expected bytes. It
// copies engine scope data into queued RHI markers and reports those scopes through the Game SDK;
// backend loading and reconstruction remain owned by the runtime.
namespace {
using Process = void(*)(void*, void*);
// Build-specific function entry guard and MinHook state. sites and hooks below share index order;
// helper/layout evidence is recorded in docs/research/ue418-hook-map.md.
struct Site {
    uint32_t rva, role;
    const char* expected;
    void* target = nullptr;
    void* original = nullptr;
};
Site sites[] = {
    {0x10b0220, RSF_AC7_ROLE_TONEMAP, "\x40\x55\x53\x56\x57\x41\x55\x41\x57\x48\x8d\x6c\x24\xb8\x48\x81"},
    {0xfbfe90, RSF_AC7_ROLE_AA, "\x40\x55\x56\x41\x56\x48\x8d\x6c\x24\xb9\x48\x81\xec\x00\x01\x00"},
    {0x1004960, RSF_AC7_ROLE_MATERIAL, "\x40\x55\x53\x56\x57\x41\x54\x41\x55\x41\x56\x48\x8d\xac\x24\xe0"},
    {0xfcbc90, RSF_AC7_ROLE_HUD, "\x40\x55\x53\x56\x41\x55\x41\x56\x41\x57\x48\x8d\xac\x24\xe8\xfe"},
    {0xfc86c0, RSF_AC7_ROLE_COMPOSITE, "\x48\x89\x5c\x24\x18\x55\x56\x57\x41\x54\x41\x55\x41\x56\x41\x57"},
    {0x10b1650, RSF_AC7_ROLE_OUTPUT, "\x40\x55\x56\x57\x48\x8d\x6c\x24\xf0\x48\x81\xec\x10\x01\x00\x00"},    {0x1098ff0, 0, "\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x40\x48\x8b\x01\x48\x8b\xda"},
    {0xfb6f20, 0, "\x40\x53\x48\x83\xec\x20\x48\x8b\x01\x48\x8b\xda\x33\xd2\xff\x50"},
    {0xff7b50, 0, "\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x40\x48\x8b\x01\x48\x8b\xfa"},
    {0xfb7bc0, 0, "\x40\x53\x48\x83\xec\x20\x48\x8b\x01\x48\x8b\xda\x33\xd2\xff\x50"},
    {0xfb7b50, 0, "\x40\x53\x48\x83\xec\x20\x48\x8b\x01\x48\x8b\xda\x33\xd2\xff\x50"},
    {0x10991e0, 0, "\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x20\x48\x8b\x01\x48\x8b\xda"},
    {0xffb900, 0, "\x4c\x8b\xdc\x55\x49\x8d\xab\xe8\xfc\xff\xff\x48\x81\xec\x10\x04"},
    {0x10b3970, 0, "\x40\x56\x57\x41\x57\x48\x83\xec\x30\x65\x48\x8b\x04\x25\x58\x00"},
    {0x112afa0, 0, "\x40\x55\x53\x41\x54\x41\x57\x48\x8d\xac\x24\x68\xff\xff\xff\x48"},
    {0x1113080, 0, "\x48\x89\x5c\x24\x20\x4c\x89\x44\x24\x18\x48\x89\x4c\x24\x08\x55"},
    {0x4d6340, 0, "\x40\x55\x57\x48\x8d\x6c\x24\xa8\x48\x81\xec\x58\x01\x00\x00\x0f"},
    {0x4d6660, 0, "\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x50\x80\x79\x78\x00\x48\x8b"},
    {0x1380d80, 0, "\x48\x8b\xc4\x48\x89\x58\x08\x48\x89\x70\x10\x48\x89\x78\x18\x4c"},
    {0x3a3f70, 0, "\x48\x8b\xc4\x48\x89\x58\x10\x48\x89\x70\x18\x55\x57\x41\x54\x41"},
    {0x113a0c0, 0, "\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x20\x48\x8b\xfa\x48\x8b\xd9"},
    {0xcea130, 0, "\x83\xb9\xb0\x01\x00\x00\x00\x48\x8b\xd1\x75\x22\xf2\x0f\x10\x82"},
    {0x3cd130, 0, "\x48\x89\x5c\x24\x10\x48\x89\x74\x24\x18\x48\x89\x7c\x24\x20\x55"},
    {0x1ab0200, 0, "\x0f\xb6\x44\x24\x28\x83\xe0\x01\x89\x91\xd0\x00\x00\x00\x44\x89"},
    {0x3c7a30, 0, "\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x30\x0f\x29\x74\x24\x20\x41"},
    {0x1781e80, 0, "\x48\x8b\xc4\x44\x88\x40\x18\x48\x89\x48\x08\x55\x53\x56\x57\x41"},
    {0x177cdd0, 0, "\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x20\x48\x8b\xd9\x0f\xb6\xfa"},
    {0x1ac3600, 0, "\x40\x55\x56\x57\x41\x54\x41\x55\x48\x8d\xac\x24\x70\xfe\xff\xff"},
    {0x1345310, 0, "\x48\x8b\xc4\x48\x89\x50\x10\x55\x53\x48\x8d\xa8\x88\xfe\xff\xff"},
    {0x133c6c0, 0, "\x40\x56\x57\x48\x83\xec\x48\x48\x89\x5c\x24\x40\x48\x8b\xf1\x48"},
    {0x1346520, 0, "\x48\x89\x5c\x24\x10\x48\x89\x6c\x24\x18\x48\x89\x74\x24\x20\x57"},
    {0x1343660, 0, "\x4c\x8b\xdc\x55\x53\x57\x49\x8d\xab\x08\xfa\xff\xff\x48\x81\xec"},
    {0x1351e10, 0, "\x48\x89\x5c\x24\x18\x55\x56\x57\x41\x55\x41\x56\x48\x83\xec\x20"},
    {0x10be2b0, 0, "\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x20\x65\x48\x8b\x04\x25\x58"},
    {0x1097d20, 0, "\x48\x89\x5c\x24\x18\x55\x56\x57\x48\x83\xec\x30\x83\xb9\x58\x02"},
    {0x109d320, 0, "\x40\x57\x48\x83\xec\x40\x83\xb9\x58\x02\x00\x00\x02\x48\x8b\xfa"},
    {0x1168d40, 0, "\x40\x55\x56\x57\x41\x56\x48\x8d\x6c\x24\xc1\x48\x81\xec\xb8\x00"},
    {0xe97d80, 0, "\x48\x89\x5c\x24\x08\x48\x89\x6c\x24\x10\x48\x89\x74\x24\x18\x57"},
    {0xe39520, 0, "\x48\x89\x5c\x24\x10\x55\x56\x57\x41\x56\x41\x57\x48\x83\xec\x20"},
    {0xe1f8d0, 0, "\x48\x89\x5c\x24\x20\x55\x41\x56\x41\x57\x48\x83\xec\x30\x0f\xb7"},
    {0xe9b880, 0, "\x48\x89\x5c\x24\x18\x48\x89\x6c\x24\x20\x56\x48\x83\xec\x20\x41"},
    {0xedb9d0, 0, "\x48\x89\x5c\x24\x20\x56\x48\x83\xec\x20\x41\x80\x78\x06\x00\x48"},
    {0x18d21c0, 0, "\x48\x89\x5c\x24\x20\x56\x48\x83\xec\x20\x41\x80\x78\x06\x00\x48"},
    {0xf4c620, 0, "\x40\x53\x56\x57\x48\x83\xec\x30\x41\x80\x78\x06\x00\x4d\x8b\xd1"},
    {0x2225470, 0, "\x48\x83\xec\x08\x80\x3d\x15\x02\x69\x01\x00\x0f\x28\xd9\xf3\x0f"},
    {0x222afa0, 0, "\x40\x55\x41\x54\x41\x55\x41\x57\x48\x8d\xac\x24\x38\xfc\xff\xff"},
    {0x10ecdd0, 0, "\x0f\xb6\x81\x1c\x0c\x00\x00\xc3\xcc\xcc\xcc\xcc\xcc\xcc\xcc\xcc"},
    {0x109f300, 0, "\x83\xb9\x58\x02\x00\x00\x02\x41\xba\x0a\x00\x00\x00\x45\x8b\xc2"},
    {0x112ff00, 0, "\x4c\x8b\xdc\x49\x89\x5b\x08\x49\x89\x6b\x18\x56\x57\x41\x56\x48"},
    {0x3a5a20, 0, "\x40\x53\x48\x83\xec\x20\xc7\x41\x08\xff\x00\x00\x00\x48\x8d\x05"},
    {0x3a8b60, 0, "\x48\x89\x5c\x24\x10\x48\x89\x6c\x24\x18\x48\x89\x74\x24\x20\x57"},
    {0x1207df0, 0, "\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x20\x48\x8b\x51\x30\x48\x8b"},
    {0x120c540, 0, "\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x20\x48\x8b\x51\x30\x48\x8b"},
    {0x1ada010, 0, "\x48\x89\x5c\x24\x08\x48\x89\x74\x24\x10\x57\x48\x83\xec\x20\x48"},
    {0xa88dc0, 0, "\x48\x83\xec\x68\x45\x33\xc9\x45\x33\xc0\x33\xd2\x84\xc9\x48\x8d"},
    {0x1adbcb0, 0, "\x40\x55\x53\x57\x48\x8d\x6c\x24\xb9\x48\x81\xec\xc0\x00\x00\x00"},
    {0xe31e90, 0, "\x48\x89\x5c\x24\x08\x48\x89\x6c\x24\x10\x48\x89\x74\x24\x18\x57"},
    {0xebf050, 0, "\x48\x89\x5c\x24\x10\x55\x56\x57\x41\x54\x41\x57\x48\x83\xec\x40"},
};
std::atomic<rsf_ac7_native_renderer*> installed{nullptr};
std::atomic<uint32_t> entry_calls{0};
std::atomic<uint32_t> outer_calls{0};
// OuterGuard spans native forwarding and post-call drains. EntryGuard counts code that can
// produce/access controller records, allowing stop to reject unloading during either interval.
struct OuterGuard {
    OuterGuard() { ++outer_calls; }
    ~OuterGuard() { --outer_calls; }
};
struct EntryGuard {
    EntryGuard() { entry_calls.fetch_add(1); }
    ~EntryGuard() { entry_calls.fetch_sub(1); }
};
// Read live engine memory defensively. Failure leaves the enclosing hook on its native path.
bool copy(void* output, const void* input, size_t size)
{
    if (!output || !input) return false;
#if defined(_MSC_VER)
    __try { std::memcpy(output, input, size); return true; }
    __except(EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION m{};
    if (!VirtualQuery(input, &m, sizeof(m)) || m.State != MEM_COMMIT ||
        (m.Protect & (PAGE_NOACCESS | PAGE_GUARD)) ||
        uintptr_t(input) + size > uintptr_t(m.BaseAddress) + m.RegionSize) return false;
    std::memcpy(output, input, size); return true;
#endif
}
template<class T> bool read(uint64_t input, size_t offset, T& output)
{ return input && copy(&output, reinterpret_cast<const void*>(uintptr_t(input) + offset), sizeof(T)); }
}
namespace { struct PassLease; }
// CPU submission identity is captured before renderer ownership crosses to render/RHI threads.
// Object addresses are registry keys; packets copy numeric identity before address retirement.
struct RendererIdentity {
    uint64_t renderer = 0, source = 0, submission = 0, viewport = 0;
    bool after_simulation = false;
    uint32_t screen = RSF_SCREEN_UNKNOWN;
    uint32_t reset = 0;
};
namespace { void log(rsf_ac7_native_renderer&, const char*); }
// Associates a Slate recording task with the exact game viewport/window/source frame.
struct WindowSource {
    uint64_t task = 0, source = 0, viewport = 0, window = 0;
    uint64_t renderer = 0, info = 0, elements = 0;
    bool after_simulation = false;
};
// Final-surface lookup is source/viewport-specific; multiple producers mark the entry ambiguous.
struct FinalSurfaceSource { uint64_t source = 0, viewport = 0, surface = 0; bool ambiguous = false; };
// Generated native uniform lifetime follows the original renderer, not temporary graph-plan scope.
struct UniformOwner { uint64_t renderer = 0, view = 0, uniform = 0; };
// Bounded PS diagnostics retain identities only, without extending engine resource lifetime.
struct PixelUniformBinding { uint64_t sequence = 0, context = 0, shader = 0, uniform = 0, command = 0; uint32_t slot = 0; };
// One installed controller. Engine identities are guarded, RHI retirements use an atomic list,
// and native pool/uniform releases are performed only by the observed render owner.
struct rsf_ac7_native_renderer {
    rsf_ac7_native_renderer_options options{};
    rsf_ac7_render_scopes* scopes = nullptr;
    std::atomic<bool> active{false};
    std::atomic<bool> quiescing{false};
    unsigned char* cloud_resolution_site = nullptr;
    unsigned char* cloud_depth_call = nullptr;
    unsigned char cloud_depth_call_bytes[7]{};
    unsigned char* cloud_depth_branch = nullptr;
    unsigned char* cloud_depth_format_site = nullptr;
    void* cloud_depth_relay = nullptr;
    ID3D11ComputeShader* cloud_depth_shader = nullptr;
    ID3D11Device* cloud_depth_device = nullptr;
    std::atomic<bool> cloud_depth_ready{false};
    std::atomic<uint32_t> cloud_depth_dispatches{0};
    std::atomic<uint32_t> cloud_depth_binding_refusals{0};
    unsigned char* cloud_motion_call = nullptr;
    void* cloud_motion_relay = nullptr;
    rsf_ac7_cloud_depth cloud_motion{};
    std::atomic<uint32_t> cloud_motion_reports{0};
    bool cloud_depth_shader_refused = false;
    unsigned char* scene_precision_site = nullptr;
    bool cloud_resolution_refused = false;
    std::mutex cloud_resolution_guard;
    uint32_t hooks = 0;
    std::atomic<uint32_t> render_thread{0};
    std::atomic<PassLease*> retired{nullptr};
    std::mutex identities_guard;
    std::array<RendererIdentity, 512> identities{};
    uint64_t next_submission = 1;
    uint32_t previous_primary_screen = RSF_SCREEN_UNKNOWN;
    uint32_t previous_screen_why = 0, screen_lines = 0;
    uint64_t previous_view_target = 0;
    std::atomic<uint32_t> live_renderers{0};
    std::atomic<uint64_t> next_source{1};
    std::array<WindowSource, 512> window_tasks{};
    std::atomic<uint32_t> live_window_tasks{0};
    struct FrameTask { void* task = nullptr; uint64_t source = 0; };
    std::array<FrameTask, 128> frame_tasks{};
    std::atomic<uint32_t> live_frame_tasks{0};
    HHOOK input_hook = nullptr;
    std::atomic<uint32_t> contact_reports{0};
    std::array<FinalSurfaceSource, 64> final_surfaces{};
    std::array<UniformOwner, 4096> uniform_owners{};
    std::atomic<uint32_t> pending_uniforms{0};
    std::mutex pixel_bindings_guard;
    std::array<PixelUniformBinding, 128> pixel_bindings{};
    uint64_t pixel_binding_sequence = 0;
    std::atomic<uint32_t> missing_pixel_reports{0};
};
namespace {
void drain_retired(rsf_ac7_native_renderer&);
void retire_renderer_uniforms(rsf_ac7_native_renderer&, void*, void*);
thread_local bool inside_engine_tick = false;
thread_local bool inside_simulation = false, after_simulation = false;
thread_local bool simulation_closed = false, simulation_started = false, input_sampled = false;
thread_local uint64_t input_source_frame = 0, reserved_source_frame = 0;
thread_local uint32_t source_render_expected = 0;
// Check the reflected inheritance chain using the researched UClass cache, including bounds.
bool object_has_class(uint64_t object, uint32_t cache_rva)
{
    uint64_t expected = 0, actual = 0, bases = 0, entry = 0;
    int32_t depth = 0, actual_depth = 0;
    const auto module = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr));
    return object && copy(&expected, module + cache_rva, 8) && expected && read(object, 0x10, actual) &&
        read(expected, 0x90, depth) && read(actual, 0x90, actual_depth) && depth >= 0 &&
        depth <= actual_depth && actual_depth <= 4096 && read(actual, 0x88, bases) &&
        read(bases, size_t(depth) * 8, entry) && entry == expected + 0x88;
}
// Copy classification with the renderer. A later world at Present cannot establish which
// screen produced this frame. Unsupported ownership, pause and camera transitions refuse FG.
// `why` is zero or class bits for an accepted screen, and a reason code in its low byte for a
// refused one: 1 unreadable player chain, 2 no view target, 3 a pending view target, 4 level
// state unreadable, 5 paused, 6 no known scene owner. Bits from 0x100 name what was found:
// 0x100 target is the hangar mesh manager, 0x200 target is the hangar pawn, 0x400 target is the
// hangar plane selector, 0x800 pawn is the hangar pawn, 0x1000 target is the pawn, 0x2000 pawn
// is the player plane, 0x4000 the world's game mode is the hangar's, 0x8000 target is the asset
// viewer manager, 0x10000 target is an engine camera actor. `view_target` returns the actor
// that owns the camera, so a change of owner can reset history.
uint32_t render_screen(uint32_t& reset, uint32_t& why, uint64_t& view_target)
{
    uint64_t engine_object = 0, client = 0, world = 0, instance = 0, players = 0;
    uint64_t player = 0, controller = 0, pawn = 0, camera = 0, target = 0, pending = 0;
    uint64_t level = 0, settings = 0, pauser = 0, hud = 0, ui = 0, layer = 0, focused = 0, mode = 0;
    int32_t count = 0; uint8_t camera_input = 0, vr = 0, cut = 0;
    view_target = 0;
    const auto module = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr));
    why = 1;
    if (!copy(&engine_object, module + 0x3cbbc28, 8) || !read(engine_object, 0x720, client) ||
        !read(client, 0x80, world) || !read(client, 0x88, instance) || !read(instance, 0x38, players) ||
        !read(instance, 0x40, count) || count != 1 || !read(players, 0, player) || !read(player, 0x30, controller) ||
        !read(controller, 0x370, pawn) || !read(controller, 0x400, camera) ||
        !read(camera, 0xfb0, target)) return RSF_SCREEN_UNKNOWN;
    why = 2; if (!target) return RSF_SCREEN_UNKNOWN;
    view_target = target;
    why = 3; if (!read(camera, 0x15a0, pending) || pending) return RSF_SCREEN_UNKNOWN;
    why = 4;
    if (!read(camera, 0x1c99, cut) || !read(world, 0x30, level) || !read(level, 0x300, settings) ||
        !read(settings, 0x4e8, pauser)) return RSF_SCREEN_UNKNOWN;
    why = 5; if (pauser) return RSF_SCREEN_UNKNOWN;
    if (cut & 4u) reset = 1;
    // UWorld::AuthorityGameMode at +0xf0 (SDK reflection dump). A wrong offset fails the
    // class check and leaves this bit clear; it cannot accept a screen by itself.
    const bool hangar_mode = read(world, 0xf0, mode) && object_has_class(mode, 0x3a82008);
    const uint32_t found = (object_has_class(target, 0x3a82130) ? 0x100u : 0u) |
        (object_has_class(target, 0x3a82140) ? 0x200u : 0u) | (object_has_class(target, 0x3a82158) ? 0x400u : 0u) |
        (object_has_class(pawn, 0x3a82140) ? 0x800u : 0u) | (target == pawn ? 0x1000u : 0u) |
        (object_has_class(pawn, 0x3a84660) ? 0x2000u : 0u) | (hangar_mode ? 0x4000u : 0u) |
        (object_has_class(target, 0x3a74730) ? 0x8000u : 0u) | (object_has_class(target, 0x3cc0150) ? 0x10000u : 0u);
    why = found;
    // The briefing owns a 3D scene. Its focused widget identifies this particular menu;
    // generic menu/video/loading frames remain unknown even when a background view exists.
    if (read(controller, 0x3f8, hud) && read(hud, 0x288, ui) && read(ui, 0xf00, layer) &&
        read(layer, 0x820, focused) && object_has_class(focused, 0x3a76c90)) return RSF_SCREEN_BRIEFING;
    // The hangar is a level with its own game mode. Its aircraft viewer hands the camera to an
    // actor that is none of the hangar classes, so the level decides, not the camera owner.
    if (found & 0x4f00u) return RSF_SCREEN_HANGAR;
    why = found | 6u;
    if (target != pawn || !object_has_class(pawn, 0x3a84660) ||
        !read(pawn, 0xe12, camera_input) || !camera_input || !read(pawn, 0xb52, vr) || vr) return RSF_SCREEN_UNKNOWN;
    why = found;
    return RSF_SCREEN_FLIGHT;
}
thread_local uint64_t submitting_viewport = 0;
// QPC timestamps describe CPU boundaries; the source ID joins later copied submission packets.
void cpu_event(rsf_ac7_native_renderer* self, uint64_t source, uint32_t stage, uint32_t kind = 0, uint32_t message = 0)
{
    if (!self || !source || !self->options.cpu_event) return;
    LARGE_INTEGER now{}, frequency{};
    QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    rsf_game_cpu_event event{sizeof(event), stage, self->options.session_id, source,
        uint64_t(now.QuadPart), uint64_t(frequency.QuadPart), source_render_expected, kind, message};
    self->options.cpu_event(self->options.user, &event);
}
thread_local std::array<uint64_t, 2> shared_repaint_targets{};
// Observe removed game-thread input messages without consuming/changing the Windows hook chain.
LRESULT CALLBACK input_message(int code, WPARAM removed, LPARAM argument)
{
    OuterGuard lifetime;
    if (code >= 0 && removed == PM_REMOVE && inside_engine_tick && input_source_frame) {
        const auto* message = reinterpret_cast<const MSG*>(argument);
        uint32_t kind = 0;
        if (message->message >= WM_KEYFIRST && message->message <= WM_KEYLAST) kind = RSF_GAME_INPUT_KEYBOARD;
        else if (message->message >= WM_MOUSEFIRST && message->message <= WM_MOUSELAST) kind = RSF_GAME_INPUT_MOUSE;
        else if (message->message == WM_INPUT) {
            RAWINPUTHEADER header{}; UINT bytes = sizeof(header);
            if (GetRawInputData(reinterpret_cast<HRAWINPUT>(message->lParam), RID_HEADER, &header, &bytes, sizeof(header)) == sizeof(header))
                kind = header.dwType == RIM_TYPEKEYBOARD ? RSF_GAME_INPUT_KEYBOARD :
                    header.dwType == RIM_TYPEMOUSE ? RSF_GAME_INPUT_MOUSE : 0;
        }
        if (kind || message->message >= 0xc000u)
            cpu_event(installed.load(std::memory_order_acquire), input_source_frame, RSF_GAME_CPU_INPUT_EVENT, kind, message->message);
    }
    return CallNextHookEx(nullptr, code, removed, argument);
}
// Admit one uncaptured primary view whose unconstrained rectangle matches host output dimensions.
bool source_primary_renderer(rsf_ac7_native_renderer& self, void* renderer)
{
    rsf_game_render_config config{}; config.struct_size = sizeof(config);
    uint64_t views = 0; int32_t count = 0, rect[4]{}; unsigned char capture[3]{};
    return self.options.render_config && self.options.render_config(self.options.user, &config) &&
        config.output_width && config.output_height && read(uint64_t(uintptr_t(renderer)), 0xb8, views) &&
        read(uint64_t(uintptr_t(renderer)), 0xc0, count) && count == 1 && read(views, 0x90, rect) &&
        read(views, 0xc42, capture) && !capture[0] && !capture[1] && !capture[2] &&
        !rect[0] && !rect[1] && rect[2] == int32_t(config.output_width) && rect[3] == int32_t(config.output_height);
}
// Reserve an identity until native renderer retirement. Capacity refusal never evicts live work.
void bind_renderer(rsf_ac7_native_renderer& self, void* object)
{
    if (!input_source_frame || !inside_engine_tick) return;
    const bool primary = submitting_viewport && after_simulation && source_primary_renderer(self, object);
    const auto key = uint64_t(uintptr_t(object));
    bool refused = false;
    char screen_text[120]{};
    {
        std::lock_guard<std::mutex> lock(self.identities_guard);
        for (const auto& slot : self.identities) if (slot.renderer == key) return;
        auto empty = std::find_if(self.identities.begin(), self.identities.end(),
            [](const RendererIdentity& entry) { return !entry.renderer; });
        if (empty == self.identities.end()) refused = true;
        else {
            uint32_t reset = 0, why = 0; uint64_t view_target = 0;
            const auto screen = primary ? render_screen(reset, why, view_target) : RSF_SCREEN_UNKNOWN;
            if (primary) {
                // A new camera owner is a hard cut even when the screen stays the same.
                if (self.previous_primary_screen != screen || self.previous_view_target != view_target) reset = 1;
                self.previous_view_target = view_target;
                // Bounded: says which rule accepted or refused the frame's screen, on change.
                if ((self.previous_primary_screen != screen || self.previous_screen_why != why) && self.screen_lines < 96) {
                    ++self.screen_lines;
                    std::snprintf(screen_text, sizeof(screen_text), "AC7 screen %u -> %u, source %llu, detail 0x%x",
                        self.previous_primary_screen, screen, static_cast<unsigned long long>(input_source_frame), why);
                }
                self.previous_primary_screen = screen; self.previous_screen_why = why;
            }
            *empty = {key, input_source_frame, self.next_submission++, submitting_viewport, after_simulation, screen, reset};
            if (primary) ++source_render_expected;
            ++self.live_renderers;
        }
    }
    if (refused && self.options.log) self.options.log(self.options.user,
        "AC7 renderer identity refused: live renderer capacity reached; no association evicted");
    if (screen_text[0] && self.options.log) self.options.log(self.options.user, screen_text);
}
// Recover the renderer key from family+0x10 and copy the CPU association into a queued packet.
void renderer_identity(rsf_ac7_native_renderer& self, rsf_game_render_pass& pass)
{
    if (pass.family_key < 0x10) return;
    std::lock_guard<std::mutex> lock(self.identities_guard);
    for (const auto& slot : self.identities) if (slot.renderer == pass.family_key - 0x10) {
        pass.source_frame_id = slot.source; pass.submission_id = slot.submission;
        pass.viewport_key = slot.viewport;
        pass.screen = slot.screen;
        if (slot.reset) pass.flags |= RSF_GAME_RENDER_RESET;
        if (slot.after_simulation) pass.flags |= RSF_GAME_RENDER_AFTER_SIMULATION;
        return;
    }
}
// Reserve one source identity around the native outer loop and restore TLS across nested ticks.
void hooked_engine_tick(void* loop)
{
    OuterGuard entry;
    const auto old_tick = inside_engine_tick;
    const auto old_source = input_source_frame, old_reserved = reserved_source_frame;
    const auto old_viewport = submitting_viewport; submitting_viewport = 0;
    const auto old_render_expected = source_render_expected; source_render_expected = 0;
    auto* self = installed.load(std::memory_order_acquire);
    reserved_source_frame = self && self->active.load() ? self->next_source.fetch_add(1) : 0;
    const auto old_repaint = shared_repaint_targets;
    const auto old_simulation = inside_simulation, old_after = after_simulation;
    const auto old_closed = simulation_closed, old_started = simulation_started, old_sampled = input_sampled;
    simulation_closed = simulation_started = input_sampled = false;
    inside_simulation = false; after_simulation = false;
    inside_engine_tick = true; input_source_frame = reserved_source_frame; shared_repaint_targets = {};
    if (self && !self->input_hook) {
        self->input_hook = SetWindowsHookExW(WH_GETMESSAGE, input_message, nullptr, GetCurrentThreadId());
        log(*self, self->input_hook ? "AC7 input markers: game-thread message dequeue and controller poll installed" :
            "AC7 input markers refused: game-thread message hook failed");
    }
    cpu_event(self, reserved_source_frame, RSF_GAME_CPU_FRAME_BEGIN);
    reinterpret_cast<void(*)(void*)>(sites[19].original)(loop);
    cpu_event(self, reserved_source_frame, RSF_GAME_CPU_FRAME_END);
    reserved_source_frame = old_reserved; submitting_viewport = old_viewport;
    inside_engine_tick = old_tick; input_source_frame = old_source; shared_repaint_targets = old_repaint;
    inside_simulation = old_simulation; after_simulation = old_after;
    simulation_closed = old_closed; simulation_started = old_started; input_sampled = old_sampled;
    source_render_expected = old_render_expected;
}
bool current_game_engine(void* object)
{
    uint64_t current = 0;
    return copy(&current, reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + 0x3cbbc28, 8) &&
        current == uint64_t(uintptr_t(object));
}
// Only the current game engine opens simulation. Completion is observed later at frame sync,
// after native recording/handoff, rather than at this function's return.
template<uint32_t Index> void hooked_simulation(void* object, float delta, uint8_t idle)
{
    OuterGuard entry;
    auto* self = installed.load(std::memory_order_acquire);
    const bool owner = self && self->active.load() && inside_engine_tick && input_source_frame &&
        !inside_simulation && current_game_engine(object);
    if (owner) {
        inside_simulation = true; after_simulation = false;
        simulation_started = true;
        cpu_event(self, input_source_frame, RSF_GAME_CPU_SIMULATION_BEGIN);
    }
    reinterpret_cast<void(*)(void*, float, uint8_t)>(sites[Index].original)(object, delta, idle);
    if (owner) {
        inside_simulation = false;
    }
}
void hooked_redraw(void* object, uint8_t present)
{
    OuterGuard entry;
    // UGameEngine::Tick enters redraw after world/viewport updates. Bind this CPU state into
    // renderer packets instead of inspecting it later from the render or RHI thread.
    if (inside_engine_tick && inside_simulation && input_source_frame && !after_simulation && current_game_engine(object)) {
        after_simulation = true;
    }
    reinterpret_cast<void(*)(void*, uint8_t)>(sites[26].original)(object, present);
}
// Associate submissions only while drawing the current engine's main viewport; nested auxiliary
// viewport draws restore the previous TLS identity when their native call returns.
void hooked_viewport_draw(void* viewport, uint8_t should_present)
{
    OuterGuard lifetime;
    const auto saved = submitting_viewport;
    submitting_viewport = 0;
    uint64_t engine_object = 0, client = 0, main_viewport = 0;
    auto* self = installed.load(std::memory_order_acquire);
    if (self && self->active.load() && inside_engine_tick && input_source_frame &&
        copy(&engine_object, reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + 0x3cbbc28, 8) &&
        read(engine_object, 0x720, client) && read(client, 0xa0, main_viewport) &&
        main_viewport == uint64_t(uintptr_t(viewport))) submitting_viewport = main_viewport;
    reinterpret_cast<void(*)(void*, uint8_t)>(sites[27].original)(viewport, should_present);
    submitting_viewport = saved;
}
// Report one input sample and the native controller-poll interval for this tick. Modal Slate loops
// are excluded from controller events because they do not own the same game input boundary.
void hooked_poll_input(void* slate)
{
    EntryGuard entry;
    auto* self = installed.load(std::memory_order_acquire);
    if (self && self->active.load() && inside_engine_tick && input_source_frame && !input_sampled) {
        input_sampled = true;
        cpu_event(self, input_source_frame, RSF_GAME_CPU_INPUT_SAMPLE);
    }
    int32_t modal_count = -1;
    if (self && self->active.load() && inside_engine_tick && input_source_frame &&
        read(uint64_t(uintptr_t(slate)), 0x1b0, modal_count) && modal_count == 0)
        cpu_event(self, input_source_frame, RSF_GAME_CPU_INPUT_EVENT, RSF_GAME_INPUT_CONTROLLER);
    reinterpret_cast<void(*)(void*)>(sites[21].original)(slate);
}
// Windows message sampling can precede device polling; whichever native boundary arrives first
// stamps the source's input sample exactly once.
void hooked_pump_messages(uint8_t main_loop)
{
    OuterGuard lifetime;
    if (main_loop && inside_engine_tick && input_source_frame && !input_sampled) {
        input_sampled = true;
        cpu_event(installed.load(std::memory_order_acquire), input_source_frame, RSF_GAME_CPU_INPUT_SAMPLE);
    }
    reinterpret_cast<void(*)(uint8_t)>(sites[54].original)(main_loop);
}
// Match the engine's actual sync instance so unrelated task waits cannot close CPU simulation.
void hooked_frame_sync(void* sync, uint8_t one_frame_lag)
{
    OuterGuard lifetime;
    const auto* engine_sync = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr)) + 0x3a4a6f8;
    if (sync == engine_sync && inside_engine_tick && input_source_frame && simulation_started && !simulation_closed) {
        simulation_closed = true;
        cpu_event(installed.load(std::memory_order_acquire), input_source_frame, RSF_GAME_CPU_SIMULATION_END);
    }
    reinterpret_cast<void(*)(void*, uint8_t)>(sites[53].original)(sync, one_frame_lag);
}
void hooked_engine_pacing(void* object)
{
    OuterGuard lifetime;
    if (inside_engine_tick && reserved_source_frame && current_game_engine(object))
        cpu_event(installed.load(std::memory_order_acquire), reserved_source_frame, RSF_GAME_CPU_PACING);
    // This runs after BeginFrame dispatch, and before PumpMessages/PollGameDeviceState.
    // Sleeping before the native time update also includes that delay in FApp's delta time.
    reinterpret_cast<void(*)(void*)>(sites[55].original)(object);
}
struct NativeShaderCode { const uint8_t* data; int32_t count, capacity; };
static_assert(sizeof(NativeShaderCode) == 16 && offsetof(NativeShaderCode, count) == 8);
// Transform only the fingerprinted contact-shadow DXBC inside the native resource-table wrapper.
// Temporary replacement bytes survive the synchronous native factory call; its result owns the shader.
void* hooked_create_pixel_shader(void* rhi, void* result, const NativeShaderCode* code)
{
    OuterGuard lifetime;
    using Create = void*(*)(void*, void*, const NativeShaderCode*);
    auto original = reinterpret_cast<Create>(sites[56].original);
    auto* self = installed.load(std::memory_order_acquire);
    NativeShaderCode input{};
    if (!self || self->quiescing.load() || !copy(&input, code, sizeof(input)) || !input.data ||
        input.count < 32 || input.count > 8 * 1024 * 1024 || input.capacity < input.count)
        return original(rhi, result, code);
    // The matched native factory reads ResourceTableBits and five uint32 arrays, then DXBC.
    // It consumes the borrowed bytes synchronously and retains only shader/table metadata.
    size_t offset = 4;
    for (uint32_t i = 0; i < 5; ++i) {
        uint32_t count = 0;
        if (offset + 4 > size_t(input.count) || !copy(&count, input.data + offset, 4) ||
            count > 65536 || size_t(count) * 4 > size_t(input.count) - offset - 4)
            return original(rhi, result, code);
        offset += 4 + size_t(count) * 4;
    }
    uint32_t optional = 0, dxbc_size = 0; char magic[4]{};
    if (offset + 32 > size_t(input.count) || !copy(magic, input.data + offset, 4) || std::memcmp(magic, "DXBC", 4) ||
        !copy(&optional, input.data + input.count - 4, 4) || optional < 4 || optional > size_t(input.count) - offset ||
        !copy(&dxbc_size, input.data + offset + 24, 4) || dxbc_size != size_t(input.count) - offset - optional)
        return original(rhi, result, code);
    std::vector<uint8_t> transformed;
    const auto patched = rsf_ac7_contact_shadow_correct(input.data + offset, size_t(input.count) - offset, transformed);
    if (patched != rsf_ac7_contact_shadow_result::patched) {
        if (patched == rsf_ac7_contact_shadow_result::refused && self->contact_reports.fetch_add(1) < 8)
            log(*self, "AC7 contact-shadow correction refused: shader fingerprint matched but transform validation failed");
        return original(rhi, result, code);
    }
    try {
        std::vector<uint8_t> complete(offset + transformed.size());
        std::memcpy(complete.data(), input.data, offset);
        std::memcpy(complete.data() + offset, transformed.data(), transformed.size());
        NativeShaderCode replacement{complete.data(), int32_t(complete.size()), int32_t(complete.size())};
        auto* created = original(rhi, result, &replacement);
        uint64_t shader = 0, native = 0;
        const bool accepted = copy(&shader, result, sizeof(shader)) && read(shader, 0xa0, native) && native;
        if (self->contact_reports.fetch_add(1) < 8) {
            char message[288]{};
            std::snprintf(message, sizeof(message),
                "AC7 contact-shadow correction: native factory resource=%u RHI=%llx D3D11=%llx CRC=%u; View noise phase and depth quantisation bias, native light/ray parameters preserved",
                accepted ? 1u : 0u, static_cast<unsigned long long>(shader), static_cast<unsigned long long>(native),
                rsf_ac7_contact_shadow_crc(transformed.data(), transformed.size()));
            log(*self, message);
        }
        return created;
    } catch (...) {
        log(*self, "AC7 contact-shadow correction refused: temporary native shader code allocation failed");
        return original(rhi, result, code);
    }
}
thread_local uint64_t frame_task_source = 0;
thread_local rsf_ac7_render_ticket* full_frame_ticket = nullptr;
// Transfer the tick source into a native BeginFrame task, then consume it at task execution.
void* hooked_frame_task_construct(void* task, void* completion, int32_t prerequisites)
{
    OuterGuard lifetime;
    auto* result = reinterpret_cast<void*(*)(void*, void*, int32_t)>(sites[49].original)(task, completion, prerequisites);
    auto* self = installed.load(std::memory_order_acquire);
    if (self && self->active.load() && inside_engine_tick && reserved_source_frame) {
        bool bound = false;
        { std::lock_guard<std::mutex> lock(self->identities_guard);
          for (auto& entry : self->frame_tasks) if (!entry.task) {
              entry = {task, reserved_source_frame}; ++self->live_frame_tasks; bound = true; break;
          } }
        if (!bound) { self->active.store(false); log(*self, "AC7 frame identity refused: BeginFrame task capacity reached"); }
    }
    return result;
}
void hooked_frame_task_execute(void* task, void* scratch, uint32_t thread)
{
    OuterGuard lifetime;
    const auto saved = frame_task_source; frame_task_source = 0;
    auto* self = installed.load(std::memory_order_acquire);
    if (self) {
        std::lock_guard<std::mutex> lock(self->identities_guard);
        for (auto& entry : self->frame_tasks) if (entry.task == task) {
            frame_task_source = entry.source; entry = {}; --self->live_frame_tasks; break;
        }
    }
    reinterpret_cast<void(*)(void*, void*, uint32_t)>(sites[50].original)(task, scratch, thread);
    frame_task_source = saved;
}
// Pair outer RHI markers with copied BeginFrame identity. Overlap deactivates admission.
void hooked_rhi_frame_begin(void* list)
{
    OuterGuard lifetime;
    auto* self = installed.load(std::memory_order_acquire);
    const auto source = frame_task_source ? frame_task_source : inside_engine_tick ? reserved_source_frame : 0;
    if (self && self->active.load() && source) {
        rsf_game_render_pass pass{}; pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_FRAME;
        pass.session_id = self->options.session_id; pass.source_frame_id = source;
        if (full_frame_ticket || !rsf_ac7_render_scope_open(self->scopes, list, &pass, &full_frame_ticket)) {
            self->active.store(false); log(*self, "AC7 full RHI frame refused: overlapping or invalid BeginFrame");
        }
    }
    reinterpret_cast<void(*)(void*)>(sites[51].original)(list);
}
// Append the matching outer end before native EndFrame. Failed queue closure disables producers
// and retains scope/module ownership rather than claiming a completed RHI interval.
void hooked_rhi_frame_end(void* list)
{
    OuterGuard lifetime;
    if (full_frame_ticket) {
        if (!rsf_ac7_render_scope_close(full_frame_ticket, list)) {
            auto* self = installed.load(std::memory_order_acquire);
            if (self) { self->active.store(false); log(*self, "AC7 full RHI frame refused: EndFrame could not queue"); }
        }
        full_frame_ticket = nullptr;
    }
    reinterpret_cast<void(*)(void*)>(sites[52].original)(list);
}
// Remove CPU identity before native address reuse, then queue generated-uniform retirement after
// native task joins. Inactive execution still drains render-owner releases during shutdown.
void hooked_renderer_retire(void* list, void* renderer)
{
    OuterGuard lifetime;
    auto* self = installed.load(std::memory_order_acquire);
    {
        EntryGuard producer;
        if (self) {
            self->render_thread.store(GetCurrentThreadId()); drain_retired(*self);
            // Remove the CPU key before native deletion can free/reuse its address. Queued
            // packets already own copied identity; late producers receive no association.
            const auto key = uint64_t(uintptr_t(renderer));
            std::lock_guard<std::mutex> lock(self->identities_guard);
            for (auto& slot : self->identities) if (slot.renderer == key) {
                slot = {}; --self->live_renderers; break;
            }
        }
    }
    reinterpret_cast<void(*)(void*, void*)>(sites[20].original)(list, renderer);
    {
        EntryGuard producer;
        // Native task/RHI waits may release the last leases synchronously. OuterGuard keeps
        // this module and its controller alive until the final render-owner drain returns.
        if (self) drain_retired(*self);
        if (self) retire_renderer_uniforms(*self, renderer, list);
    }
}
// Queue a primary-view submission interval for CPU/GPU joins; family identity alone is insufficient.
void hooked_render_family(void* list, void* renderer)
{
    OuterGuard lifetime;
    auto* self = installed.load(std::memory_order_acquire);
    rsf_ac7_render_ticket* ticket = nullptr;
    if (self && self->active.load()) {
        rsf_game_render_pass pass{}; pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_SUBMISSION;
        pass.session_id = self->options.session_id; pass.family_key = uint64_t(uintptr_t(renderer)) + 0x10;
        pass.pass_key = uint64_t(uintptr_t(renderer)); pass.flags = RSF_GAME_RENDER_PRIMARY;
        renderer_identity(*self, pass);
        // The scope contract requires the same concrete primary view used by SR. A family
        // key alone refuses open and cannot produce a completion marker for the CPU join.
        read(uint64_t(uintptr_t(renderer)), 0xb8, pass.view_key);
        if (pass.view_key) {
            read(pass.view_key, 0x70, pass.render_rect);
            read(pass.view_key, 0x80, pass.output_rect);
        }
        uint32_t native_frame = 0;
        if (read(pass.family_key, 0x68, native_frame)) pass.native_frame = native_frame;
        if (pass.source_frame_id && pass.viewport_key && (pass.flags & RSF_GAME_RENDER_AFTER_SIMULATION) &&
            source_primary_renderer(*self, renderer) &&
            !rsf_ac7_render_scope_open(self->scopes, list, &pass, &ticket))
            log(*self, "AC7 submission scope refused: no RHI completion marker will be queued");
    }
    reinterpret_cast<void(*)(void*, void*)>(sites[48].original)(list, renderer);
    if (ticket && !rsf_ac7_render_scope_close(ticket, list)) {
        self->active.store(false); log(*self, "AC7 submission source deactivated: RHI scope could not close");
    }
}

// Engine pooled targets are retained on the render owner, COM surfaces through RHI execution.
// End execution releases COM refs and queues the engine refs/uniforms for render-owner retirement.
struct PassLease {
    rsf_ac7_native_renderer* owner = nullptr;
    PassLease* next = nullptr;
    uint64_t input_pool = 0, depth_pool = 0, exposure_pool = 0;
    ID3D11Resource* motion = nullptr;
    ID3D11Resource* ui = nullptr;
    ID3D11Resource* scene_surface = nullptr;
    std::atomic<uint64_t> output_pool{0};
    std::vector<uint64_t> uniforms;
};
using GetPointer = void*(*)(void*, uint32_t);
using Ref = uint32_t(*)(void*);
// Query the reviewed postprocess output slot and its pooled-target reference, still on CPU recording.
uint64_t output_pool(uint64_t node, uint32_t index)
{
    uint64_t table = 0, method = 0;
    if (!read(node, 0, table) || !read(table, 0x38, method) || !method) return 0;
    auto* output = reinterpret_cast<GetPointer>(uintptr_t(method))(reinterpret_cast<void*>(uintptr_t(node)), index);
    uint64_t pool = 0; read(uint64_t(uintptr_t(output)), 0x50, pool); return pool;
}
// Native pool virtuals: +0x28 retains, +0x30 releases. Call only under engine-owner lifetime rules.
void pool_ref(uint64_t pool, uint32_t offset)
{
    uint64_t table = 0, method = 0;
    if (read(pool, 0, table) && read(table, offset, method) && method)
        reinterpret_cast<Ref>(uintptr_t(method))(reinterpret_cast<void*>(uintptr_t(pool)));
}
// Resolve targetable (+8) or readable (+16) RHI texture to its borrowed D3D11 resource.
void* pool_texture(uint64_t pool, bool target)
{
    uint64_t texture = 0, table = 0, method = 0;
    if (!read(pool, target ? 8 : 16, texture) || !texture) read(pool, 8, texture);
    if (!read(texture, 0, table) || !read(table, 0x30, method) || !method) return nullptr;
    using GetNative = void*(*)(void*);
    return reinterpret_cast<GetNative>(uintptr_t(method))(reinterpret_cast<void*>(uintptr_t(texture)));
}
// RHI execution refreshes output allocation, which may not exist when begin was recorded.
void resolve_pass(void* object, rsf_ac7_render_scope* scope)
{
    auto& lease = *static_cast<PassLease*>(object);
    scope->color_input = pool_texture(lease.input_pool, false);
    scope->color_output = pool_texture(lease.output_pool.load(std::memory_order_acquire), true);
    scope->color_output_readable = pool_texture(lease.output_pool.load(std::memory_order_acquire), false);
    scope->depth = pool_texture(lease.depth_pool, false); scope->motion = lease.motion;
    scope->exposure = pool_texture(lease.exposure_pool, false);
    scope->ui_input = lease.ui; scope->scene_surface = lease.scene_surface;
}
// RHI-safe COM release; defer engine pool/uniform destruction to its observed render thread.
void release_pass(void* object)
{
    auto* lease = static_cast<PassLease*>(object);
    if (lease->motion) { lease->motion->Release(); lease->motion = nullptr; }
    if (lease->ui) { lease->ui->Release(); lease->ui = nullptr; }
    if (lease->scene_surface) { lease->scene_surface->Release(); lease->scene_surface = nullptr; }
    auto& retired = lease->owner->retired;
    auto* head = retired.load();
    do { lease->next = head; } while (!retired.compare_exchange_weak(head, lease));
}
// Detach the entire atomic retire list only on the native render owner. Inactive observers keep
// calling this during quiescence, so an asynchronous end marker can eventually release its lease.
void drain_retired(rsf_ac7_native_renderer& self)
{
    if (self.render_thread.load() != GetCurrentThreadId()) return;
    auto* lease = self.retired.exchange(nullptr);
    while (lease) {
        auto* next = lease->next;
        pool_ref(lease->input_pool, 0x30); pool_ref(lease->depth_pool, 0x30); pool_ref(lease->exposure_pool, 0x30); pool_ref(lease->output_pool.load(), 0x30);
        using MoveRef = void*(*)(void*, void*);
        auto release = reinterpret_cast<MoveRef>(reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + 0xde5cf0);
        for (auto& uniform : lease->uniforms) {
            uint64_t empty = 0; release(&uniform, &empty);
        }
        delete lease; lease = next;
    }
}
void log(rsf_ac7_native_renderer& self, const char* message)
{ if (self.options.log) self.options.log(self.options.user, message); }

// Reviewed UE4.18 graph reference/output layouts; raw descriptor bytes preserve the native ABI.
struct NativeRef { uint64_t node = 0; uint32_t index = 0, padding = 0; };
struct NativeOutput { alignas(8) unsigned char descriptor[0x50]{}; uint64_t pool = 0; uint32_t dependencies = 0, padding = 0; };
static_assert(sizeof(NativeRef) == 0x10 && sizeof(NativeOutput) == 0x60);
uint64_t method(uint64_t node, uint32_t slot)
{
    uint64_t table = 0, entry = 0;
    if (!read(node, 0, table) || !read(table, slot, entry)) return 0;
    return entry;
}
NativeRef* input_ref(uint64_t node, uint32_t index = 0)
{
    const auto entry = method(node, 8);
    return entry ? static_cast<NativeRef*>(reinterpret_cast<GetPointer>(uintptr_t(entry))(
        reinterpret_cast<void*>(uintptr_t(node)), index)) : nullptr;
}
NativeOutput* node_output(uint64_t node, uint32_t index = 0)
{
    const auto entry = method(node, 0x38);
    return entry ? static_cast<NativeOutput*>(reinterpret_cast<GetPointer>(uintptr_t(entry))(
        reinterpret_cast<void*>(uintptr_t(node)), index)) : nullptr;
}
void* native_texture(uint64_t texture)
{
    const auto entry = method(texture, 0x30);
    return entry ? reinterpret_cast<void*(*)(void*)>(uintptr_t(entry))(
        reinterpret_cast<void*>(uintptr_t(texture))) : nullptr;
}
void hooked_translucency_size(void* scene, uint8_t downsample)
{
    EntryGuard entry;
    reinterpret_cast<void(*)(void*, uint8_t)>(sites[33].original)(scene, downsample);
    auto* self = installed.load(std::memory_order_acquire);
    // Ordinary 3D translucency owns this scale. The UnmodifiedTranslucency UI producer
    // allocates from BufferSize +0x208 under its own output-size view/depth scope.
    static bool reported = false;
    if (self && self->active.load() && !reported) {
        reported = true;
        log(*self, "native ordinary translucency: engine sizing retained; full-resolution UI uses its scoped producer");
    }
}
#if defined(RSF_AC7_GRAPH_TEST)
void* (*test_engine)(uint32_t) = nullptr;
#endif
template<class Fn> Fn engine(uint32_t rva)
{
#if defined(RSF_AC7_GRAPH_TEST)
    if (test_engine) return reinterpret_cast<Fn>(test_engine(rva));
#endif
    return reinterpret_cast<Fn>(reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + rva);
}
// Retain generated uniform data until the owning renderer's queued commands have executed.
bool retain_generated_uniform(rsf_ac7_native_renderer& self, uint64_t view, uint64_t uniform)
{
    uint64_t family = 0;
    if (!uniform || !read(view, 0, family) || family < 0x10) return false;
    std::lock_guard<std::mutex> lock(self.identities_guard);
    const auto slot = std::find_if(self.uniform_owners.begin(), self.uniform_owners.end(),
        [](const UniformOwner& entry) { return !entry.uniform; });
    if (slot == self.uniform_owners.end()) return false;
    InterlockedIncrement(reinterpret_cast<volatile LONG*>(uintptr_t(uniform) + 8));
    *slot = {family - 0x10, view, uniform}; ++self.pending_uniforms;
    return true;
}
// Transfer this renderer's retained uniforms into one end-marker lease. Failed admission/closure
// deactivates new work and leaves the references owned until safe cleanup becomes possible.
void retire_renderer_uniforms(rsf_ac7_native_renderer& self, void* renderer, void* list) try
{
    const uint64_t key = uint64_t(uintptr_t(renderer));
    std::lock_guard<std::mutex> lock(self.identities_guard);
    const auto count = std::count_if(self.uniform_owners.begin(), self.uniform_owners.end(),
        [key](const UniformOwner& entry) { return entry.renderer == key && entry.uniform; });
    if (!count) return;
    auto* lease = new(std::nothrow) PassLease;
    if (!lease) { self.active.store(false); return; }
    lease->owner = &self;
    try { lease->uniforms.reserve(size_t(count)); } catch (...) { delete lease; throw; }
    rsf_ac7_render_scope scope{}; scope.struct_size = sizeof(scope);
    scope.session_id = self.options.session_id; scope.family_key = key + 0x10;
    for (const auto& entry : self.uniform_owners) if (entry.renderer == key && entry.uniform) {
        scope.view_key = entry.view; lease->uniforms.push_back(entry.uniform);
    }
    // Native retirement waits for CPU recording tasks and dispatches their lists. A marker
    // appended afterward retires raw uniform pointers only after earlier RHI commands execute.
    // The scope carries numeric identity only; no retired FViewInfo memory is read here.
    rsf_ac7_render_ticket* ticket = nullptr;
    const rsf_ac7_scope_lease owned{lease, nullptr, release_pass};
    if (!rsf_ac7_render_scope_open_leased(self.scopes, list, &scope, &owned, &ticket)) {
        delete lease; self.active.store(false);
        log(self, "AC7 uniform retirement refused; generated buffers and module retained"); return;
    }
    if (!rsf_ac7_render_scope_close(ticket, list)) {
        self.active.store(false);
        log(self, "AC7 uniform retirement could not close; buffers and module retained"); return;
    }
    for (auto& entry : self.uniform_owners) if (entry.renderer == key && entry.uniform) {
        entry = {}; --self.pending_uniforms;
    }
}
catch (...) {
    self.active.store(false); log(self, "AC7 uniform retirement failed; generated buffers and module retained");
}
using OneArg = void(*)(void*);
using GetScene = void*(*)();
struct GraphPlan;
// Private native graph node, registered for engine-owned Release. The virtual-table order and
// descriptor offsets mirror the researched upscale node; plan is borrowed only during processing.
struct SRNode {
    void** table = nullptr;
    unsigned char flags[8]{};
    NativeRef inputs[2]{};
    NativeOutput output{};
    unsigned char unused[0x28]{};
    uint32_t quality = 1;
    float panini[3]{0, 0, 1};
    int32_t extent[2]{};
    GraphPlan* plan = nullptr;
    std::array<NativeRef, 16> auxiliary{};
    uint32_t auxiliary_count = 0;
    NativeRef exposure_ref{};
    uint64_t preceding_exposure_pool = 0;
};
static_assert(offsetof(SRNode, output) == 0x30 && offsetof(SRNode, quality) == 0xb8);
// Render-thread transaction for inserting SR and temporarily resizing reviewed downstream passes.
// Saved engine bytes/uniform refs restore before the CPU call returns. RHI packets never borrow plan.
struct GraphPlan {
    rsf_ac7_native_renderer* owner = nullptr;
    rsf_ac7_render_scope packet{};
    uint64_t velocity = 0, depth_pool = 0;
    std::vector<uint64_t> consumers;
    int32_t saved_rect[4]{}, saved_buffer[2]{};
    std::array<unsigned char, 0x380> saved_matrices{};
    std::array<unsigned char, 0xcf0> saved_parameters{};
    uint64_t cached_parameters = 0, saved_uniform = 0;
    void* scene = nullptr;
    SRNode* node = nullptr;
    bool resized = false;
    int32_t surface_extent[2]{};
    bool contains(uint64_t pass) const
    { return std::find(consumers.begin(), consumers.end(), pass) != consumers.end(); }
};
thread_local GraphPlan* graph_plan = nullptr;
thread_local uint64_t postprocess_view = 0, postprocess_velocity = 0;
// Minimal native graph virtuals: two indexed scene inputs, one output and bounded auxiliary
// dependencies. Unused name/fence/setter slots return the native ABI's empty representation.
void node_release(SRNode* node)
{ pool_ref(node->output.pool, 0x30); delete node; }
void* node_destruct(SRNode* node, uint32_t flags)
{ if (flags & 1) node_release(node); return node; }
void* node_input(SRNode* node, uint32_t index)
{ return index < 2 ? &node->inputs[index] : nullptr; }
void node_set(SRNode* node, uint32_t index, const NativeRef* ref)
{ if (index < 2 && ref) node->inputs[index] = *ref; }
void node_add(SRNode*, const NativeRef*) {}
bool node_blends(SRNode*) { return false; }
void* sr_output(SRNode* node, uint32_t index) { return index ? nullptr : &node->output; }
void* node_dependency(SRNode* node, uint32_t index)
{ return index == 0 ? &node->inputs[0] : index <= node->auxiliary_count ? &node->auxiliary[index - 1] : nullptr; }
void* node_none(SRNode*, uint32_t) { return nullptr; }
void* node_string(SRNode*, uint32_t) { static uint64_t empty[2]{}; return empty; }
void node_set_string(SRNode*, uint32_t, const void*) {}
void* node_fence(SRNode*) { return nullptr; }
// Preserve the source descriptor except output extent, FP16 format, samples and target flags.
void* node_descriptor(SRNode* node, void* result, uint32_t)
{
    const auto* source = node_output(node->inputs[0].node, node->inputs[0].index);
    if (!source) { std::memset(result, 0, 0x50); return result; }
    std::memcpy(result, source->descriptor, 0x50);
    auto* bytes = static_cast<unsigned char*>(result);
    std::memcpy(bytes + 0x14, node->extent, 8);
    const uint32_t format = 10, flags = 1; // PF_FloatRGBA, TexCreate_RenderTargetable.
    const uint16_t samples = 1;
    std::memcpy(bytes + 0x2c, &format, 4); std::memcpy(bytes + 0x34, &flags, 4);
    std::memcpy(bytes + 0x28, &samples, 2); bytes[0x38] = 0; bytes[0x48] = 1; bytes[0x49] = 0;
    const wchar_t* name = L"ReScaleFrameSR";
    std::memcpy(bytes + 0x40, &name, 8);
    return result;
}
using MoveUniform = void*(*)(void*, void*);
void wait_for_view_recorders()
{
    // Native WaitForOutstandingTasksOnly joins CPU command recording before shared view edits.
    engine<void(*)()>(0x12183c0)();
}
uint64_t retain_view_uniform(unsigned char* view, size_t offset)
{
    uint64_t uniform = 0;
    copy(&uniform, view + offset, sizeof(uniform));
    if (uniform) InterlockedIncrement(reinterpret_cast<volatile LONG*>(uintptr_t(uniform) + 8));
    return uniform;
}
// Rebuild complete native shader parameters after sizing changes and publish an owned uniform.
void build_postprocess_uniform(GraphPlan& plan, unsigned char* view)
{
    alignas(16) unsigned char bounds[0x40]{};
    // Invoke the pure parameter producer, not InitRHIResources' dynamic-resource initialization.
    engine<void(*)(void*, void*, void*, void*, void*, uint32_t, void*)>(0x11346d0)(view, plan.scene,
        view + 0xc0, view + 0x21e0, bounds, 2, reinterpret_cast<void*>(uintptr_t(plan.cached_parameters)));
    uint64_t dynamic = 0, create = 0, uniform = 0;
    copy(&dynamic, engine<void*>(0x3c783d0), 8); create = method(dynamic, 0xf0);
    if (!create) return;
    using CreateUniform = void*(*)(void*, uint64_t*, const void*, const void*, uint32_t);
    reinterpret_cast<CreateUniform>(uintptr_t(create))(reinterpret_cast<void*>(uintptr_t(dynamic)), &uniform,
        reinterpret_cast<void*>(uintptr_t(plan.cached_parameters)), engine<void*>(0x3caa6c8), 1);
    if (!uniform) return;
    if (!retain_generated_uniform(*plan.owner, plan.packet.view_key, uniform)) {
        uint64_t empty = 0; engine<MoveUniform>(0xde5cf0)(&uniform, &empty);
        log(*plan.owner, "AC7 generated view uniform refused: renderer lifetime capacity unavailable"); return;
    }
    // Save an owned reference without clearing the shared slot. A worker can queue that slot.
    // Move only from the local replacement, so publication never exposes an intermediate null.
    plan.saved_uniform = retain_view_uniform(view, 0x10);
    engine<MoveUniform>(0xde5cf0)(view + 0x10, &uniform);
}
// Join native CPU recording tasks before mutating the shared view/scene and its derived matrices.
void resize_consumers(GraphPlan& plan)
{
    wait_for_view_recorders();
    auto* view = reinterpret_cast<unsigned char*>(uintptr_t(plan.packet.view_key));
    auto* scene = static_cast<unsigned char*>(plan.scene);
    copy(plan.saved_rect, view + 0x70, 16); copy(plan.saved_buffer, scene + 0x208, 8);
    copy(plan.saved_matrices.data(), view + 0xc0, plan.saved_matrices.size());
    copy(plan.saved_parameters.data(), reinterpret_cast<void*>(uintptr_t(plan.cached_parameters)), plan.saved_parameters.size());
    std::memcpy(view + 0x70, plan.packet.output_rect, 16);
    const int32_t extent[] = {plan.surface_extent[0], plan.surface_extent[1]};
    std::memcpy(scene + 0x208, extent, 8);
    // Let the engine rebuild every derived shader value, rather than rewriting a GPU buffer.
    engine<OneArg>(0xff9af0)(view + 0xc0);
    build_postprocess_uniform(plan, view);
    plan.resized = true;
}
void node_process(SRNode* node, void* context)
{
    auto& plan = *node->plan;
    // A complete native bilinear result exists even if SR refuses on the RHI thread. This uses
    // the engine's own draw/resolve commands and never bypasses later grading or UI processing.
    reinterpret_cast<Process>(sites[5].original)(node, context);
    auto* lease = new(std::nothrow) PassLease;
    if (lease) {
        lease->owner = plan.owner;
        lease->input_pool = output_pool(node->inputs[0].node, node->inputs[0].index);
        lease->depth_pool = plan.depth_pool; lease->output_pool.store(node->output.pool);
        lease->exposure_pool = node->preceding_exposure_pool ? node->preceding_exposure_pool :
            output_pool(node->exposure_ref.node, node->exposure_ref.index);
        pool_ref(lease->input_pool, 0x28); pool_ref(lease->depth_pool, 0x28); pool_ref(lease->exposure_pool, 0x28); pool_ref(node->output.pool, 0x28);
        // FPostProcessing receives a pooled render target reference, not an RHI texture.
        lease->motion = static_cast<ID3D11Resource*>(pool_texture(plan.velocity, false));
        if (lease->motion) lease->motion->AddRef();
        uint64_t list = 0; read(uint64_t(uintptr_t(context)), 0x28, list);
        rsf_ac7_render_ticket* ticket = nullptr;
        const rsf_ac7_scope_lease owned{lease, resolve_pass, release_pass};
        if (rsf_ac7_render_scope_open_leased(plan.owner->scopes, reinterpret_cast<void*>(uintptr_t(list)),
            &plan.packet, &owned, &ticket)) {
            if (!rsf_ac7_render_scope_close(ticket, reinterpret_cast<void*>(uintptr_t(list)))) {
                plan.owner->active.store(false); log(*plan.owner, "AC7 native SR scope close refused; module retained");
            }
        } else release_pass(lease);
    }
    resize_consumers(plan);
}
void* sr_table[] = {
    reinterpret_cast<void*>(&node_destruct), reinterpret_cast<void*>(&node_input), reinterpret_cast<void*>(&node_input),
    reinterpret_cast<void*>(&node_set), reinterpret_cast<void*>(&node_add), reinterpret_cast<void*>(&node_process),
    reinterpret_cast<void*>(&node_blends), reinterpret_cast<void*>(&sr_output), reinterpret_cast<void*>(&node_dependency),
    reinterpret_cast<void*>(&node_none), reinterpret_cast<void*>(&node_string), reinterpret_cast<void*>(&node_set_string),
    reinterpret_cast<void*>(&node_none), reinterpret_cast<void*>(&node_set_string), reinterpret_cast<void*>(&node_descriptor),
    reinterpret_cast<void*>(&node_release), reinterpret_cast<void*>(&node_fence)
};
bool primary_view(uint64_t, const rsf_game_render_config&);
// Bounded dependency search for the unique named SceneColorHalfRes consumer of this scene.
// Ambiguity, cycles exceeding bounds or incomplete accessors refuse rewiring.
NativeRef* bloom_scene_input(uint64_t bloom, const NativeRef& scene)
{
    std::vector<uint64_t> pending{bloom}, seen;
    NativeRef* result = nullptr;
    while (!pending.empty() && seen.size() < 128) {
        const auto node = pending.back(); pending.pop_back();
        if (!node || std::find(seen.begin(), seen.end(), node) != seen.end()) continue;
        seen.push_back(node);
        if (method(node, 0x70) == uint64_t(uintptr_t(engine<void*>(0xfb7a00)))) {
            uint64_t name = 0; wchar_t label[18]{};
            if (read(node, 0xc8, name) && copy(label, reinterpret_cast<void*>(uintptr_t(name)), sizeof(label)) &&
                std::memcmp(label, L"SceneColorHalfRes", sizeof(label)) == 0) {
                auto* input = input_ref(node);
                if (!input || input->node != scene.node || input->index != scene.index || result) return nullptr;
                result = input;
            }
        }
        const auto dependency = method(node, 0x40);
        if (!dependency) return nullptr;
        bool bounded = false;
        for (uint32_t i = 0; i <= 16; ++i) {
            const auto* ref = static_cast<const NativeRef*>(reinterpret_cast<GetPointer>(uintptr_t(dependency))(
                reinterpret_cast<void*>(uintptr_t(node)), i));
            if (!ref) { bounded = true; break; }
            if (ref->node) pending.push_back(ref->node);
        }
        if (!bounded) return nullptr;
    }
    return pending.empty() ? result : nullptr;
}
// Validate primary camera, jitter, resources and every downstream pass before graph insertion.
// Once registered, the engine owns the new node; only copied packet data outlives this CPU plan.
bool make_plan(rsf_ac7_native_renderer& self, void* context, void* root, GraphPlan& plan) try
{
    rsf_game_render_config config{}; config.struct_size = sizeof(config);
    if (!self.options.render_config || !self.options.render_config(self.options.user, &config) || !config.enabled ||
        !config.output_width || !config.output_height || config.output_width > 16384 || config.output_height > 16384) return false;
    auto& p = plan.packet; p.struct_size = sizeof(p); p.session_id = self.options.session_id; p.role = RSF_AC7_ROLE_SR;
    uint32_t frame = 0; uint64_t cached = 0, uniform = 0;
    const auto ctx = uint64_t(uintptr_t(context));
    if (!read(ctx, 0, p.view_key) || p.view_key != postprocess_view || !read(p.view_key, 0, p.family_key) ||
        !read(p.family_key, 0x68, frame) || !read(p.view_key, 0x70, p.render_rect) ||
        !read(p.view_key, 0x80, p.output_rect) || !read(p.view_key, 0x1418, cached) || !cached || !read(p.view_key, 0x10, uniform) || !uniform ||
        p.render_rect[0] < 0 || p.render_rect[1] < 0 || p.render_rect[2] <= p.render_rect[0] || p.render_rect[3] <= p.render_rect[1] ||
        p.output_rect[0] < 0 || p.output_rect[1] < 0 || p.output_rect[2] <= p.output_rect[0] || p.output_rect[3] <= p.output_rect[1] ||
        p.output_rect[2] > int32_t(config.output_width) || p.output_rect[3] > int32_t(config.output_height)) return false;
    if (!primary_view(p.view_key, config)) return false;
    std::array<unsigned char, RSF_AC7_VIEW_BUFFER_BYTES> bytes{};
    rsf_ac7_view view{}; view.struct_size = sizeof(view);
    if (!copy(bytes.data(), reinterpret_cast<void*>(uintptr_t(cached)), 0xcf0) ||
        rsf_ac7_view_read(bytes.data(), uint32_t(bytes.size()), RSF_AC7_VIEW_ABI_VERSION, &view) != RSF_AC7_VIEW_OK ||
        !view.has_jitter || view.view_width != uint32_t(p.render_rect[2] - p.render_rect[0]) ||
        view.view_height != uint32_t(p.render_rect[3] - p.render_rect[1])) return false;
    uint64_t cursor = uint64_t(uintptr_t(root)), tone = 0;
    for (uint32_t count = 0; cursor && count < 64; ++count) {
        if (plan.contains(cursor)) return false;
        const auto entry = method(cursor, 0x28);
        const auto found = std::find_if(std::begin(sites), std::begin(sites) + 6,
            [entry](const Site& site) { return uint64_t(uintptr_t(site.target)) == entry; });
        if (found == std::begin(sites) + 6) return false; // No unreviewed downstream pass is resized.
        plan.consumers.push_back(cursor);
        if (found->role == RSF_AC7_ROLE_TONEMAP) { tone = cursor; break; }
        auto* input = input_ref(cursor); if (!input) return false; cursor = input->node;
    }
    auto* input = tone ? input_ref(tone) : nullptr;
    if (!input || !input->node) return false;
    plan.surface_extent[0] = int32_t(config.output_width); plan.surface_extent[1] = int32_t(config.output_height);
    plan.cached_parameters = cached;
    plan.scene = engine<GetScene>(0x109dc70)(); plan.owner = &self; plan.velocity = postprocess_velocity;
    if (!read(uint64_t(uintptr_t(plan.scene)), 0x60, plan.depth_pool) || !plan.depth_pool || !plan.velocity) return false;
    auto* node = new(std::nothrow) SRNode;
    if (!node) return false;
    node->table = sr_table; node->inputs[0] = node->inputs[1] = *input;
    const auto* bloom = input_ref(tone, 1);
    auto* bloom_input = bloom && bloom->node ? bloom_scene_input(bloom->node, *input) : nullptr;
    if (bloom_input) {
        // Current exposure depends on reconstructed colour too. Use already-owned engine
        // exposure preceding this update, avoiding a cycle back through the new SR node.
        const auto state = engine<uint64_t(*)(void*)>(0x1126090)(reinterpret_cast<void*>(uintptr_t(p.view_key)));
        int32_t index = -1;
        if (read(state, 0xbd8, index) && index >= 0 && index <= 1)
            read(state, 0xbe0 + size_t(index) * 8, node->preceding_exposure_pool);
    } else if (const auto* exposure = input_ref(tone, 2)) node->exposure_ref = *exposure;
    node->extent[0] = int32_t(config.output_width); node->extent[1] = int32_t(config.output_height); node->plan = &plan;
    const auto dependency = method(tone, 0x40);
    if (!dependency) { delete node; return false; }
    // The graph processes dependencies in input order. Finish all original tonemap branches
    // before SR changes view/scene sizes, including auxiliary and additional dependencies.
    bool bounded = false;
    for (uint32_t i = 0; !bloom_input && i <= 16; ++i) {
        const auto* ref = static_cast<const NativeRef*>(reinterpret_cast<GetPointer>(uintptr_t(dependency))(
            reinterpret_cast<void*>(uintptr_t(tone)), i));
        if (!ref) { bounded = true; break; }
        if (ref->node && !(ref->node == input->node && ref->index == input->index)) {
            if (node->auxiliary_count == node->auxiliary.size()) break;
            node->auxiliary[node->auxiliary_count++] = *ref;
        }
    }
    if (!bloom_input && !bounded) { delete node; return false; }
    // Register through the native graph owner so it retires this heap node through Release.
    engine<void*(*)(void*, void*)>(0xe93670)(static_cast<unsigned char*>(context) + 0x18, node);
    input->node = uint64_t(uintptr_t(node)); input->index = 0;
    if (bloom_input) {
        bloom_input->node = uint64_t(uintptr_t(node)); bloom_input->index = 0;
        static uint32_t reports = 0;
        if (reports++ < 6) log(self, "native SR ordering: SceneColorHalfRes, bloom and exposure now consume reconstructed colour");
    }
    plan.node = node; p.pass_key = uint64_t(uintptr_t(node)); p.native_frame = frame;
    read(p.view_key, 8, p.history_key);
    // UE ordinary written NDC motion converts to UV by half extents and a Y flip. The packet's
    // camera-included flag remains zero because unwritten pixels need depth-derived camera motion.
    p.motion_to_uv[0] = 0.5f; p.motion_to_uv[1] = -0.5f; p.motion_camera_included = 0;
    p.camera_valid = 1; p.camera.struct_size = sizeof(p.camera); p.camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    p.camera.render_width = view.view_width; p.camera.render_height = view.view_height;
    p.camera.output_width = config.output_width; p.camera.output_height = config.output_height;
    p.camera.near_plane = view.near_plane; p.camera.vertical_fov_radians = view.vertical_fov; p.camera.depth_inverted = 1;
    std::memcpy(p.camera.view_to_clip, view.view_to_clip_no_jitter, 64);
    std::memcpy(p.camera.clip_to_view, view.clip_to_view_no_jitter, 64);
    renderer_identity(self, p); p.flags |= RSF_GAME_RENDER_PRIMARY;
    std::memcpy(p.camera.clip_to_previous_clip, view.clip_to_prev_clip, 64);
    std::memcpy(p.previous_clip_to_clip, view.prev_clip_to_clip, 64);
    std::memcpy(p.jitter_pixels, view.jitter_pixels, 8); std::memcpy(p.previous_jitter_pixels, view.previous_jitter_pixels, 8);
    std::memcpy(p.camera.jitter_pixels, view.jitter_pixels, 8); std::memcpy(p.camera.previous_jitter_pixels, view.previous_jitter_pixels, 8);
    const float* axes[] = {view.camera_right, view.camera_up, view.camera_forward};
    for (uint32_t row = 0; row < 3; ++row) for (uint32_t col = 0; col < 3; ++col) {
        p.camera.view_to_world[row * 4 + col] = axes[row][col];
        p.camera.world_to_view[col * 4 + row] = axes[row][col];
        p.camera.world_to_view[12 + row] -= view.camera_position[col] * axes[row][col];
    }
    std::memcpy(p.camera.view_to_world + 12, view.camera_position, 12);
    p.camera.view_to_world[15] = p.camera.world_to_view[15] = 1;
    unsigned char cut = 0; read(p.view_key, 0xc34, cut); if (cut) p.flags |= RSF_GAME_RENDER_RESET;
    return true;
}
catch (...) { return false; }
// Execute one admitted graph transaction under TLS, then join workers and restore every borrowed
// view/scene field. The engine retires its registered node independently of this stack-local plan.
void hooked_context(void* context, void* root)
{
    const EntryGuard guard; auto* self = installed.load(); GraphPlan plan;
    auto original = reinterpret_cast<Process>(sites[13].original);
    if (!self || !self->active.load() || !make_plan(*self, context, root, plan)) { original(context, root); return; }
    auto* previous = graph_plan; graph_plan = &plan;
    original(context, root);
    graph_plan = previous;
    if (plan.resized) {
        wait_for_view_recorders();
        auto* view = reinterpret_cast<unsigned char*>(uintptr_t(plan.packet.view_key));
        std::memcpy(view + 0x70, plan.saved_rect, 16);
        std::memcpy(static_cast<unsigned char*>(plan.scene) + 0x208, plan.saved_buffer, 8);
        std::memcpy(view + 0xc0, plan.saved_matrices.data(), plan.saved_matrices.size());
        std::memcpy(reinterpret_cast<void*>(uintptr_t(plan.cached_parameters)), plan.saved_parameters.data(), plan.saved_parameters.size());
        if (plan.saved_uniform) engine<MoveUniform>(0xde5cf0)(view + 0x10, &plan.saved_uniform);
    }
    // GraphPlan is CPU-only. Queued packets contain copies and pool leases, never this pointer.
}
// Temporary output-size view/depth selection for AC7's UnmodifiedTranslucency UI producer.
// Both uniform selectors and all saved view/scene fields must be restored before leaving its scope.
struct UIProducerState {
    GraphPlan plan;
    uint64_t depth = 0, scaled_depth = 0;
    uint64_t saved_scaled_uniform = 0;
    bool scaled_uniform_bound = false;
    bool active = false;
};
thread_local UIProducerState ui_producer;
thread_local void* translucency_renderer = nullptr;
// Join native workers before restoring both uniform selectors, view matrices/parameters and
// scene depth/extent. Release only the temporary pool refs retained by prepare_ui_producer.
void restore_ui_producer()
{
    auto& state = ui_producer;
    if (!state.active) return;
    wait_for_view_recorders();
    auto& plan = state.plan;
    auto* view = reinterpret_cast<unsigned char*>(uintptr_t(plan.packet.view_key));
    auto* scene = static_cast<unsigned char*>(plan.scene);
    std::memcpy(scene + 0x60, &state.depth, sizeof(state.depth));
    std::memcpy(scene + 0x208, plan.saved_buffer, sizeof(plan.saved_buffer));
    std::memcpy(view + 0x70, plan.saved_rect, sizeof(plan.saved_rect));
    std::memcpy(view + 0xc0, plan.saved_matrices.data(), plan.saved_matrices.size());
    std::memcpy(reinterpret_cast<void*>(uintptr_t(plan.cached_parameters)), plan.saved_parameters.data(), plan.saved_parameters.size());
    if (state.scaled_uniform_bound) {
        engine<MoveUniform>(0xde5cf0)(view + 0x18, &state.saved_scaled_uniform);
        state.scaled_uniform_bound = false;
    }
    if (plan.saved_uniform) engine<MoveUniform>(0xde5cf0)(view + 0x10, &plan.saved_uniform);
    pool_ref(state.depth, 0x30); pool_ref(state.scaled_depth, 0x30);
    state.depth = state.scaled_depth = 0; state.active = false; plan.resized = false;
}
// Populate enlarged native depth first, then borrow it while rebuilding the output-size UI view.
bool prepare_ui_producer(rsf_ac7_native_renderer& self, void* scene, void* list, void* view)
{
    rsf_game_render_config config{}; config.struct_size = sizeof(config);
    auto& state = ui_producer; auto& plan = state.plan;
    const auto key = uint64_t(uintptr_t(view));
    int32_t buffer[2]{}, rect[4]{}; uint64_t cached = 0, uniform = 0, depth = 0;
    if (state.active || !translucency_renderer || !self.active.load() || !self.options.render_config ||
        !self.options.render_config(self.options.user, &config) || !config.enabled ||
        !primary_view(key, config) || !read(key, 0x80, rect) || !read(key, 0x1418, cached) || !cached ||
        !read(key, 0x10, uniform) || !uniform || !read(uint64_t(uintptr_t(scene)), 0x60, depth) || !depth ||
        !read(uint64_t(uintptr_t(scene)), 0x208, buffer) || buffer[0] <= 0 || buffer[1] <= 0 ||
        rect[0] < 0 || rect[1] < 0 || rect[2] <= rect[0] || rect[3] <= rect[1] ||
        rect[2] > int32_t(config.output_width) || rect[3] > int32_t(config.output_height)) return false;
    // Allocate and populate the engine's own enlarged depth before replacing its borrowed
    // scene-depth binding. The depth producer still sees the original scene-sized view.
    const int32_t extent[] = {int32_t(config.output_width), int32_t(config.output_height)};
    uint64_t packed_extent = 0; std::memcpy(&packed_extent, extent, sizeof(packed_extent));
    engine<void*(*)(void*, void*, uint64_t)>(0x109e0d0)(scene, list, packed_extent);
    uint64_t scaled_depth = 0;
    if (!read(uint64_t(uintptr_t(scene)), 0x1b8, scaled_depth) || !scaled_depth) return false;
    const float scale = std::max(float(config.output_width) / float(buffer[0]), float(config.output_height) / float(buffer[1]));
    if (!std::isfinite(scale) || scale < 1.0f || scale > 4.0f) return false;
    engine<void(*)(void*, void*, void*, void*, float, uint8_t)>(0xee93f0)(translucency_renderer,
        list, reinterpret_cast<void*>(uintptr_t(scaled_depth + 8)), view, scale, 0);
    plan.owner = &self; plan.scene = scene; plan.packet.view_key = key; plan.cached_parameters = cached;
    std::memcpy(plan.packet.output_rect, rect, sizeof(rect));
    std::memcpy(plan.surface_extent, extent, sizeof(extent));
    state.depth = depth; state.scaled_depth = scaled_depth;
    pool_ref(depth, 0x28); pool_ref(scaled_depth, 0x28);
    resize_consumers(plan);
    if (!plan.saved_uniform) {
        // Restoration must remain possible even if native uniform creation refused.
        state.active = true; restore_ui_producer(); return false;
    }
    // Patched translucent material policies can select the scaled-view slot even for this
    // game-specific UI pass. It may be empty when no ordinary translucency pass preceded it.
    // Both selectors must receive this pass's complete output-size unjittered view.
    uint64_t current_uniform = 0;
    if (!read(key, 0x10, current_uniform) || !current_uniform) {
        state.active = true; restore_ui_producer(); return false;
    }
    state.saved_scaled_uniform = retain_view_uniform(static_cast<unsigned char*>(view), 0x18);
    InterlockedIncrement(reinterpret_cast<volatile LONG*>(uintptr_t(current_uniform) + 8));
    engine<MoveUniform>(0xde5cf0)(static_cast<unsigned char*>(view) + 0x18, &current_uniform);
    state.scaled_uniform_bound = true;
    std::memcpy(static_cast<unsigned char*>(scene) + 0x60, &scaled_depth, sizeof(scaled_depth));
    state.active = true;
    static uint32_t reports = 0;
    if (reports++ < 6) log(self, "native UnmodifiedTranslucency: output-size allocation, depth and unjittered view before rasterization");
    return true;
}
// Begin the native UI producer under an output-size view/depth transaction; an unsuccessful native
// begin restores immediately, while successful work restores at resolve or enclosing pass exit.
uint64_t hooked_unmodified_begin(void* scene, void* list, void* view, uint8_t clear, uint8_t glow, uint8_t extra)
{
    EntryGuard entry; auto* self = installed.load();
    const bool prepared = self && prepare_ui_producer(*self, scene, list, view);
    const auto result = reinterpret_cast<uint64_t(*)(void*, void*, void*, uint8_t, uint8_t, uint8_t)>(sites[34].original)(
        scene, list, view, clear, glow, extra);
    if (prepared && !(result & 0xffu)) restore_ui_producer();
    return result;
}
void hooked_unmodified_resolve(void* scene, void* list, void* view, uint8_t glow, uint8_t extra)
{
    EntryGuard entry;
    reinterpret_cast<void(*)(void*, void*, void*, uint8_t, uint8_t)>(sites[35].original)(scene, list, view, glow, extra);
    if (ui_producer.active && ui_producer.plan.scene == scene && ui_producer.plan.packet.view_key == uint64_t(uintptr_t(view)))
        restore_ui_producer();
}
// FSceneRenderTargets::GetSceneColor: the shading path at +0x25c and the alpha/format selection
// at +0x260/+0x230 pick SceneColor[Mobile/HighEnd/HighEndWithAlpha] at +0x30/+0x38/+0x40,
// matching AC7_FSceneRenderTargets_AllocateSceneColor (RVA 0x1095010). Unknown paths refuse.
uint64_t scene_colour_pool(void* scene)
{
    int32_t path = 0, format = 0; uint8_t alpha = 0; uint64_t pool = 0;
    const auto targets = uint64_t(uintptr_t(scene));
    if (!read(targets, 0x25c, path) || !read(targets, 0x260, alpha) || !read(targets, 0x230, format)) return 0;
    const size_t offset = path == 0 ? 0x30 : path == 1 ? (alpha || format == 10 ? 0x40 : 0x38) : 0;
    return offset && read(targets, offset, pool) ? pool : 0;
}
// Identity of the single primary view of a renderer that SR reconstructs, or false.
bool primary_scope(rsf_ac7_native_renderer& self, void* renderer, uint32_t role, rsf_ac7_render_scope& scope)
{
    rsf_game_render_config config{}; config.struct_size = sizeof(config);
    if (!self.active.load() || !self.options.render_config ||
        !self.options.render_config(self.options.user, &config) || !config.enabled) return false;
    uint64_t storage = 0; int32_t count = 0; uint32_t frame = 0;
    scope = {}; scope.struct_size = sizeof(scope); scope.role = role; scope.session_id = self.options.session_id;
    if (!read(uint64_t(uintptr_t(renderer)), 0xb8, storage) || !read(uint64_t(uintptr_t(renderer)), 0xc0, count) ||
        count != 1 || !read(storage, 0, scope.family_key) || !read(scope.family_key, 0x68, frame) ||
        !read(storage, 0x70, scope.render_rect) || !read(storage, 0x80, scope.output_rect) ||
        !primary_view(storage, config)) return false;
    scope.view_key = storage; scope.native_frame = frame;
    return true;
}
// Bracket passes 0 (standard), 1 (after-DOF layer +0x1b0, allocator 0x109f4d0), and 2 (all).
// Pass 4 is AC7's UI layer and stays outside. Lease scene colour for host before/after comparison.
rsf_ac7_render_ticket* open_translucency(rsf_ac7_native_renderer& self, void* renderer, void* list,
    uint32_t pass, void* scene, PassLease*& out)
{
    out = nullptr;
    rsf_ac7_render_scope scope{};
    if (pass > 2 || !scene || !primary_scope(self, renderer, RSF_GAME_RENDER_TRANSLUCENCY, scope)) return nullptr;
    scope.pass_key = pass;
    scope.flags = RSF_GAME_RENDER_PRIMARY | (pass == 1 ? RSF_GAME_RENDER_TRANSLUCENCY_LAYER : 0u);
    const auto colour = scene_colour_pool(scene);
    if (!colour) return nullptr;
    auto* lease = new(std::nothrow) PassLease;
    if (!lease) return nullptr;
    lease->owner = &self; lease->input_pool = colour; pool_ref(colour, 0x28);
    rsf_ac7_render_ticket* ticket = nullptr;
    const rsf_ac7_scope_lease owned{lease, resolve_pass, release_pass};
    if (!rsf_ac7_render_scope_open_leased(self.scopes, list, &scope, &owned, &ticket)) {
        pool_ref(colour, 0x30); delete lease; return nullptr;
    }
    out = lease; return ticket;
}
// RenderBasePass, RVA 0xebf050 (DeferredShadingRenderer.cpp:954): the material draws of the
// primary view, including parallel lists submitted before it returns, inside one queued scope.
uint8_t hooked_base_pass(void* renderer, void* list, uint32_t access)
{
    OuterGuard lifetime;
    auto* self = installed.load(std::memory_order_acquire);
    rsf_ac7_render_scope scope{};
    rsf_ac7_render_ticket* ticket = nullptr;
    if (self && primary_scope(*self, renderer, RSF_GAME_RENDER_MATERIALS, scope) &&
        !rsf_ac7_render_scope_open(self->scopes, list, &scope, &ticket)) ticket = nullptr;
    const auto result = reinterpret_cast<uint8_t(*)(void*, void*, uint32_t)>(sites[57].original)(renderer, list, access);
    if (ticket && !rsf_ac7_render_scope_close(ticket, list)) log(*self, "AC7 base pass scope could not close");
    return result;
}
// Populate any missing native scaled-view selector, bracket eligible scene translucency, then
// publish an on-demand layer before end. Restore a UI transaction even when native Resolve is skipped.
void hooked_translucency_render(void* renderer, void* list, uint32_t pass)
{
    OuterGuard lifetime;
    auto* previous = translucency_renderer; translucency_renderer = renderer;
    auto* self = installed.load();
    rsf_game_render_config config{}; config.struct_size = sizeof(config);
    uint64_t storage = 0; int32_t count = 0;
    auto* scene = engine<GetScene>(0x109dc70)(); float layer_scale = 1;
    read(uint64_t(uintptr_t(scene)), 0x220, layer_scale);
    if (self && self->active.load() && self->options.render_config &&
        self->options.render_config(self->options.user, &config) && config.enabled && layer_scale != 1.0f &&
        read(uint64_t(uintptr_t(renderer)), 0xb8, storage) && read(uint64_t(uintptr_t(renderer)), 0xc0, count) &&
        count > 0 && count <= 16) {
        for (int32_t i = 0; i < count; ++i) {
            const uint64_t view = storage + uint64_t(i) * 0x27c0;
            uint64_t scaled_uniform = 0, parameters = 0, normal_uniform = 0;
            if (!read(view, 0x1418, parameters) || !parameters || !read(view, 0x10, normal_uniform) || !normal_uniform)
                continue;
            if (read(view, 0x18, scaled_uniform) && scaled_uniform) continue;
            // The stock producer is conditional on the ordinary translucency pass. AC7's
            // unmodified/extra passes can select this slot without taking that branch first.
            engine<void(*)(void*, void*, void*)>(0x116ea00)(renderer, list, reinterpret_cast<void*>(uintptr_t(view)));
            if (!read(view, 0x18, scaled_uniform) || !scaled_uniform) {
                log(*self, "AC7 translucency pass refused: native scaled-view uniform producer returned no buffer");
                translucency_renderer = previous; return;
            }
            static uint32_t reports = 0;
            if (reports++ < 6) log(*self, "native translucency: populated missing scaled-view uniform before material selection");
        }
    }
    PassLease* lease = nullptr;
    auto* ticket = self ? open_translucency(*self, renderer, list, pass, scene, lease) : nullptr;
    reinterpret_cast<void(*)(void*, void*, uint32_t)>(sites[36].original)(renderer, list, pass);
    if (ticket) {
        // The layer is allocated on demand inside the pass. Publish it before end is queued.
        uint64_t layer = 0;
        if (pass == 1 && read(uint64_t(uintptr_t(scene)), 0x1b0, layer) && layer) {
            pool_ref(layer, 0x28); lease->output_pool.store(layer, std::memory_order_release);
        }
        if (!rsf_ac7_render_scope_close(ticket, list)) log(*self, "AC7 translucency scope could not close");
        static uint32_t reports = 0;
        if (reports++ < 3) log(*self, "native translucency: scene colour leased around passes 0/1/2 for reactive and transparency inputs");
    }
    // Stereo/empty branches may omit Resolve. Do not leave a changed view after this producer.
    if (ui_producer.active) restore_ui_producer();
    translucency_renderer = previous;
}
// Diagnostic observation of null View uniform producers; preserve native enqueue arguments.
void hooked_pixel_view_uniform(void* shader, void* list, void* rhi_shader, void* uniform)
{
    EntryGuard entry;
    if (!uniform) {
        auto* self = installed.load();
        static std::atomic<uint32_t> reports{0};
        if (self && self->active.load() && reports.fetch_add(1) < 8) {
            uintptr_t caller = 0;
#if defined(_MSC_VER)
            caller = uintptr_t(_ReturnAddress());
#elif defined(__GNUC__)
            caller = uintptr_t(__builtin_return_address(0));
#endif
            const auto base = uintptr_t(GetModuleHandleW(nullptr));
            char message[224]{};
            std::snprintf(message, sizeof(message),
                "native null pixel View uniform enqueue: caller RVA 0x%llx, shader %p, UI scope %u, renderer %p, thread %lu",
                static_cast<unsigned long long>(caller >= base ? caller - base : 0), shader,
                ui_producer.active ? 1u : 0u, translucency_renderer, static_cast<unsigned long>(GetCurrentThreadId()));
            log(*self, message);
        }
    }
    reinterpret_cast<void(*)(void*, void*, void*, void*)>(sites[37].original)(shader, list, rhi_shader, uniform);
}
// Remember recent binding identities without retaining/dereferencing them on a future callback.
void hooked_rhi_pixel_uniform(void* context, void* shader, uint32_t slot, void* uniform)
{
    EntryGuard entry; auto* self = installed.load();
    if (self && self->active.load()) {
        uint64_t command = 0;
        read(uint64_t(uintptr_t(GetModuleHandleW(nullptr))), 0x3c78358, command);
        std::lock_guard<std::mutex> lock(self->pixel_bindings_guard);
        const auto sequence = ++self->pixel_binding_sequence;
        self->pixel_bindings[sequence % self->pixel_bindings.size()] = {sequence,
            uint64_t(uintptr_t(context)) - 0x18, uint64_t(uintptr_t(shader)),
            uint64_t(uintptr_t(uniform)), command, slot};
    }
    reinterpret_cast<void(*)(void*, void*, uint32_t, void*)>(sites[38].original)(context, shader, slot, uniform);
}
// Bounded evidence for required dirty PS uniform slots. Observations never repair native state.
void hooked_rhi_pixel_tables(void* context, void* shader)
{
    EntryGuard entry; auto* self = installed.load();
    uint32_t required = 0; uint16_t dirty = 0; uint32_t missing = 0;
    const auto key = uint64_t(uintptr_t(context));
    if (self && self->active.load() && read(uint64_t(uintptr_t(shader)), 0x30, required) &&
        read(key, 0x40e6, dirty)) {
        const auto selected = required & dirty;
        for (uint32_t slot = 0; slot < 14; ++slot) if (selected & (1u << slot)) {
            uint64_t uniform = 0;
            if (read(key, 0x3f90 + size_t(slot) * 8, uniform) && !uniform) missing |= 1u << slot;
        }
        if (missing && self->missing_pixel_reports.fetch_add(1) < 4) {
            uint64_t hashes = 0;
            read(uint64_t(uintptr_t(shader)), 0x68, hashes);
            char message[224]{};
            std::snprintf(message, sizeof(message), "native missing PS uniform table: context %p shader %p required 0x%x dirty 0x%x missing 0x%x",
                context, shader, required, uint32_t(dirty), missing); log(*self, message);
            for (uint32_t slot = 0; slot < 14; ++slot) if (missing & (1u << slot)) {
                uint32_t expected_hash = 0; read(hashes, size_t(slot) * 4, expected_hash);
                std::snprintf(message, sizeof(message), "native missing PS slot %u expected layout hash 0x%x", slot, expected_hash);
                log(*self, message);
            }
            std::array<PixelUniformBinding, 16> recent{};
            {
                std::lock_guard<std::mutex> lock(self->pixel_bindings_guard);
                for (uint32_t i = 0; i < recent.size(); ++i) {
                    if (self->pixel_binding_sequence > i) {
                        const auto sequence = self->pixel_binding_sequence - i;
                        recent[i] = self->pixel_bindings[sequence % self->pixel_bindings.size()];
                    }
                }
            }
            for (const auto& binding : recent) if (binding.sequence) {
                std::snprintf(message, sizeof(message), "native preceding PS bind: seq %llu context %llx shader %llx slot %u uniform %llx command %llx",
                    static_cast<unsigned long long>(binding.sequence), static_cast<unsigned long long>(binding.context),
                    static_cast<unsigned long long>(binding.shader), binding.slot, static_cast<unsigned long long>(binding.uniform),
                    static_cast<unsigned long long>(binding.command)); log(*self, message);
            }
        }
    }
    // Evidence only: preserve the native failure rather than hiding an incomplete material draw.
    reinterpret_cast<void(*)(void*, void*)>(sites[39].original)(context, shader);
}
// Decode the reviewed enqueue variants only to diagnose PS slot-one null ownership. Variant 43
// exposes its uniform in the appended command, so observe it after the original enqueue.
template<uint32_t Index> void hooked_pixel_enqueue(void* list, void* shader, void* parameter, void* input)
{
    EntryGuard entry; auto* self = installed.load();
    uint8_t bound = 0; uint16_t slot = 0;
    const bool selected = self && self->active.load() &&
        read(uint64_t(uintptr_t(parameter)), 6, bound) && bound &&
        read(uint64_t(uintptr_t(parameter)), 4, slot) && slot == 1;
    uint64_t uniform = uint64_t(uintptr_t(input));
    if (Index == 41) read(uint64_t(uintptr_t(input)), 0x38, uniform);
    if (Index == 42) read(uint64_t(uintptr_t(input)), 0, uniform);
    auto original = reinterpret_cast<void(*)(void*, void*, void*, void*)>(sites[Index].original);
    if (Index == 43) {
        original(list, shader, parameter, input);
        uint64_t command = 0;
        if (!selected || !read(uint64_t(uintptr_t(list)), 8, command) || !read(command, 0x20, uniform)) return;
    }
    if (selected && !uniform) {
        static std::atomic<uint32_t> reports{0};
        if (reports.fetch_add(1) < 4) {
            void* frames[10]{};
            const auto count = CaptureStackBackTrace(0, 10, frames, nullptr);
            const auto base = uint64_t(uintptr_t(GetModuleHandleW(nullptr)));
            char message[224]{};
            std::snprintf(message, sizeof(message), "native null PS slot1 CPU creator: RVA 0x%x shader %p parameter %p input %p UI %u renderer %p",
                sites[Index].rva, shader, parameter, input, ui_producer.active ? 1u : 0u, translucency_renderer);
            log(*self, message);
            for (uint32_t i = 0; i < count; ++i) {
                const auto frame = uint64_t(uintptr_t(frames[i]));
                std::snprintf(message, sizeof(message), "native null PS CPU stack %u: address %llx game RVA %llx",
                    i, static_cast<unsigned long long>(frame),
                    static_cast<unsigned long long>(frame >= base && frame - base < 0x5000000 ? frame - base : 0));
                log(*self, message);
            }
        }
    }
    if (Index != 43) original(list, shader, parameter, input);
}
// Keep CPU postprocess view/velocity TLS and lease the concrete primary family output. Later Slate
// sampling validates this exact source/viewport/surface association instead of selecting by size.
void hooked_postprocess(void* owner, void* list, void* view, void* velocity_ref)
{
    const EntryGuard guard; auto* self = installed.load();
    const auto old_view = postprocess_view, old_velocity = postprocess_velocity;
    postprocess_view = uint64_t(uintptr_t(view)); read(uint64_t(uintptr_t(velocity_ref)), 0, postprocess_velocity);
    if (self) { self->render_thread.store(GetCurrentThreadId()); drain_retired(*self); }
    rsf_ac7_render_ticket* final_ticket = nullptr;
    if (self && self->active.load()) {
        rsf_game_render_config config{}; config.struct_size = sizeof(config);
        rsf_ac7_render_scope scope{}; scope.struct_size = sizeof(scope);
        scope.session_id = self->options.session_id; scope.role = RSF_GAME_RENDER_FINAL_SCENE;
        scope.view_key = uint64_t(uintptr_t(view)); scope.pass_key = scope.view_key;
        uint32_t frame = 0; uint64_t target = 0;
        if (self->options.render_config && self->options.render_config(self->options.user, &config) &&
            primary_view(scope.view_key, config) && read(scope.view_key, 0, scope.family_key) &&
            read(scope.family_key, 0x68, frame) && read(scope.family_key, 0x20, target) &&
            read(scope.view_key, 0x70, scope.render_rect) && read(scope.view_key, 0x80, scope.output_rect)) {
            scope.native_frame = frame; scope.flags = RSF_GAME_RENDER_PRIMARY;
            renderer_identity(*self, scope);
            // The native HUD pass uses this same FRenderTarget virtual slot to access the family
            // output. Query it on the render owner, then retain only its underlying D3D surface.
            const auto getter = method(target, 8);
            uint64_t texture = 0;
            if (getter) {
                auto* ref = reinterpret_cast<void*(*)(void*)>(uintptr_t(getter))(reinterpret_cast<void*>(uintptr_t(target)));
                read(uint64_t(uintptr_t(ref)), 0, texture);
            }
            auto* lease = new(std::nothrow) PassLease;
            if (lease) {
                lease->owner = self; lease->scene_surface = static_cast<ID3D11Resource*>(native_texture(texture));
                if (lease->scene_surface) {
                    lease->scene_surface->AddRef();
                    if (scope.source_frame_id && scope.viewport_key) {
                        std::lock_guard<std::mutex> lock(self->identities_guard);
                        auto& key = self->final_surfaces[scope.source_frame_id % self->final_surfaces.size()];
                        if (key.source == scope.source_frame_id) key.ambiguous = true;
                        else key = {scope.source_frame_id, scope.viewport_key,
                            uint64_t(uintptr_t(lease->scene_surface)), false};
                    }
                    const rsf_ac7_scope_lease owned{lease, resolve_pass, release_pass};
                    if (!rsf_ac7_render_scope_open_leased(self->scopes, list, &scope, &owned, &final_ticket)) release_pass(lease);
                } else delete lease;
            }
        }
    }
    reinterpret_cast<void(*)(void*, void*, void*, void*)>(sites[12].original)(owner, list, view, velocity_ref);
    if (final_ticket && !rsf_ac7_render_scope_close(final_ticket, list)) {
        self->active.store(false); log(*self, "AC7 final scene source deactivated: native RHI scope could not close");
    }
    postprocess_view = old_view; postprocess_velocity = old_velocity;
}
// Exclude captures/stereo auxiliaries using native capture flags and the unconstrained output rect.
bool primary_view(uint64_t view, const rsf_game_render_config& config)
{
    int32_t unconstrained[4]{}; unsigned char capture[3]{};
    return read(view, 0x90, unconstrained) && read(view, 0xc42, capture) &&
        !capture[0] && !capture[1] && !capture[2] && unconstrained[0] == 0 && unconstrained[1] == 0 &&
        unconstrained[2] == int32_t(config.output_width) && unconstrained[3] == int32_t(config.output_height);
}
// Commit exact backend dimensions to owned primary view rectangles before engine allocation.
void prepare_owned_views(void* renderer, const rsf_game_render_config& config)
{
    uint64_t storage = 0; int32_t count = 0; bool changed = false;
    const uint32_t width = config.enabled ? config.render_width : config.output_width;
    const uint32_t height = config.enabled ? config.render_height : config.output_height;
    if (!width || !height || width > config.output_width || height > config.output_height ||
        config.output_width > 16384 || config.output_height > 16384 ||
        !read(uint64_t(uintptr_t(renderer)), 0xb8, storage) || !read(uint64_t(uintptr_t(renderer)), 0xc0, count) ||
        count < 1 || count > 16) return;
    for (int32_t i = 0; i < count; ++i) {
        const auto view = storage + uint64_t(i) * 0x27c0;
        uint32_t mode = 0; uint64_t state = 0; int32_t rect[4]{};
        if (!read(view, 0x80, rect) || !read(view, 0x13c0, mode) || !read(view, 8, state) || mode != 1 || !state ||
            !primary_view(view, config) || rect[0] < 0 || rect[1] < 0 || rect[2] <= rect[0] || rect[3] <= rect[1]) continue;
        // Keep the active rectangle at the backend's exact size. The engine pads allocations
        // separately; Ultra Performance can accept a single width that is not divisible by four.
        const auto scale = [](int32_t value, uint32_t input, uint32_t output) {
            return int32_t((uint64_t(value) * input + output / 2) / output);
        };
        const int32_t x = scale(rect[0], width, config.output_width), y = scale(rect[1], height, config.output_height);
        const int32_t scaled[4]{x, y,
            x + scale(rect[2] - rect[0], width, config.output_width),
            y + scale(rect[3] - rect[1], height, config.output_height)};
        std::memcpy(reinterpret_cast<void*>(uintptr_t(view + 0x70)), scaled, sizeof(scaled)); changed = true;
    }
    if (changed) engine<OneArg>(0x19b6fe0)(static_cast<unsigned char*>(renderer) + 0x10);
}
void* hooked_construct(void* renderer, void* family, void* hit_proxy)
{
    const EntryGuard guard;
    auto* result = reinterpret_cast<void*(*)(void*, void*, void*)>(sites[15].original)(renderer, family, hit_proxy);
    auto* self = installed.load(); rsf_game_render_config config{}; config.struct_size = sizeof(config);
    if (self && self->active.load()) {
        bind_renderer(*self, renderer);
        if (self->options.render_config && self->options.render_config(self->options.user, &config))
            prepare_owned_views(renderer, config);
    }
    return result;
}
// Current visibility-call temporal states; byte-sized native sample fields limit phases to 255.
struct TemporalSampleOwner { uint64_t state = 0; uint32_t frame = 0, phases = 8; };
thread_local std::array<TemporalSampleOwner, 16> temporal_sample_owners;
uint32_t hooked_temporal_sample_index(void* state)
{
    EntryGuard entry;
    for (const auto& owner : temporal_sample_owners) {
        if (owner.state && owner.state == uint64_t(uintptr_t(state))) {
            const auto index = uint8_t(owner.frame % owner.phases), phases = uint8_t(owner.phases);
            // Preserve the native jitter producer and history matrices, replacing its fixed
            // eight-sample setting at the view-state boundary where it consumes the phase.
            std::memcpy(static_cast<unsigned char*>(state) + 0xc1c, &index, 1);
            std::memcpy(static_cast<unsigned char*>(state) + 0xc1d, &phases, 1);
            return index;
        }
    }
    return reinterpret_cast<uint32_t(*)(void*)>(sites[46].original)(state);
}
// Borrow temporal AA mode only for native visibility/jitter preparation. Restore mode afterward
// so later native pass selection is unchanged; refuse mismatched/padded allocations during resize.
void hooked_visibility(void* renderer, void* list, uintptr_t third, void* fourth)
{
    const EntryGuard guard; auto* self = installed.load();
    std::array<uint64_t, 16> views{}; std::array<uint32_t, 16> modes{}; uint32_t selected = 0;
    const auto previous_samples = temporal_sample_owners; temporal_sample_owners = {};
    rsf_game_render_config config{}; config.struct_size = sizeof(config);
    uint64_t storage = 0; int32_t count = 0; int32_t buffer[2]{};
    auto* scene = engine<GetScene>(0x109dc70)();
    read(uint64_t(uintptr_t(scene)), 0x208, buffer);
    if (self && self->active.load() && self->options.render_config &&
        self->options.render_config(self->options.user, &config) && config.enabled &&
        read(uint64_t(uintptr_t(renderer)), 0xb8, storage) && read(uint64_t(uintptr_t(renderer)), 0xc0, count) &&
        count > 0 && count <= 16) {
        for (int32_t i = 0; i < count; ++i) {
            const uint64_t view = storage + uint64_t(i) * 0x27c0;
            int32_t rect[4]{}; uint32_t mode = 0; uint64_t state = 0;
            if (read(view, 0x80, rect) && read(view, 0x13c0, mode) && read(view, 8, state) && state && mode == 1 &&
                primary_view(view, config) && rect[0] >= 0 && rect[1] >= 0 && rect[2] > rect[0] && rect[3] > rect[1]) {
                int32_t render[4]{}; read(view, 0x70, render);
                const bool cropped = rect[0] || rect[1] || rect[2] != int32_t(config.output_width) || rect[3] != int32_t(config.output_height);
                const uint32_t expected_width = (config.render_width + 3) & ~3u;
                const uint32_t expected_height = (config.render_height + 3) & ~3u;
                const bool allocated = buffer[0] == int32_t(expected_width) && buffer[1] == int32_t(expected_height) &&
                    (cropped || (render[2] - render[0] == int32_t(config.render_width) &&
                                 render[3] - render[1] == int32_t(config.render_height)));
                if (!allocated || render[2] > buffer[0] || render[3] > buffer[1]) {
                    // Engine buffer hysteresis can retain a larger allocation after a scale
                    // change. Render a complete unjittered native frame until allocation and
                    // viewport agree; never feed padded bloom/depth regions to the SR graph.
                    if (buffer[0] >= rect[2] && buffer[1] >= rect[3])
                        std::memcpy(reinterpret_cast<void*>(uintptr_t(view + 0x70)), rect, 16);
                    continue;
                }
                views[selected] = view; modes[selected++] = mode;
                uint64_t temporal_state = 0, family = 0; uint32_t frame = 0;
                if (read(view, 0x1410, temporal_state) && temporal_state && read(view, 0, family) &&
                    read(family, 0x68, frame)) {
                    const auto phases = uint32_t(std::clamp(std::round(8.0 * double(config.output_width) * config.output_height /
                        (double(render[2] - render[0]) * (render[3] - render[1]))), 8.0, 255.0));
                    temporal_sample_owners[selected - 1] = {temporal_state, frame, phases};
                    static uint32_t last_phases = 0;
                    if (last_phases != phases) {
                        last_phases = phases;
                        char message[112]{}; std::snprintf(message, sizeof(message),
                            "native temporal sampling: %u phases for %ux%u from %dx%d", phases,
                            config.output_width, config.output_height, render[2] - render[0], render[3] - render[1]);
                        log(*self, message);
                    }
                }
                const uint32_t temporal = 2; std::memcpy(reinterpret_cast<void*>(uintptr_t(view + 0x13c0)), &temporal, 4);
            }
        }
    }
    // Select the stock temporal preparation only here. DOF/SSR and native AA graph selection see
    // their original modes afterward. Engine histories and derived projection values stay native.
    reinterpret_cast<void(*)(void*, void*, uintptr_t, void*)>(sites[14].original)(renderer, list, third, fourth);
    for (uint32_t i = 0; i < selected; ++i)
        std::memcpy(reinterpret_cast<void*>(uintptr_t(views[i] + 0x13c0)), &modes[i], 4);
    temporal_sample_owners = previous_samples;
}
// Override allocation extent only for explicitly reviewed consumers in the current CPU graph plan.
void* descriptor(uint32_t index, void* node, void* out, uint32_t output)
{
    const EntryGuard guard;
    auto* result = reinterpret_cast<void*(*)(void*, void*, uint32_t)>(sites[index + 6].original)(node, out, output);
    if (graph_plan && graph_plan->contains(uint64_t(uintptr_t(node)))) {
        const int32_t extent[] = {graph_plan->surface_extent[0], graph_plan->surface_extent[1]};
        std::memcpy(static_cast<unsigned char*>(result) + 0x14, extent, 8);
    }
    return result;
}
template<uint32_t I> void* hooked_descriptor(void* node, void* out, uint32_t output)
{ return descriptor(I, node, out, output); }
// Bracket reviewed native postprocess work in queued scopes and publish its output before close.
// For the rewired AA node, forward its input pool instead of applying a second reconstruction.
void process(uint32_t index, void* node, void* context)
{
    const EntryGuard guard;
    auto* self = installed.load(std::memory_order_acquire);
    auto original = reinterpret_cast<Process>(sites[index].original);
    if (self) {
        self->render_thread.store(GetCurrentThreadId());
        drain_retired(*self);
    }
    if (!self || !self->active.load(std::memory_order_acquire)) { original(node, context); return; }
    rsf_ac7_render_scope scope{};
    scope.struct_size = sizeof(scope); scope.session_id = self->options.session_id;
    scope.role = sites[index].role; scope.pass_key = uint64_t(uintptr_t(node));
    uint64_t list = 0; uint32_t frame = 0;
    const auto ctx = uint64_t(uintptr_t(context));
    const bool valid = read(ctx, 0, scope.view_key) && read(ctx, 0x28, list) &&
        read(scope.view_key, 0, scope.family_key) && read(scope.family_key, 0x68, frame) &&
        read(scope.view_key, 0x70, scope.render_rect) && read(scope.view_key, 0x80, scope.output_rect) &&
        read(scope.view_key, 0xab4, scope.jitter_pixels);
    scope.native_frame = frame;
    if (valid) {
        renderer_identity(*self, scope);
        rsf_game_render_config config{}; config.struct_size = sizeof(config);
        if (self->options.render_config && self->options.render_config(self->options.user, &config) &&
            primary_view(scope.view_key, config)) scope.flags |= RSF_GAME_RENDER_PRIMARY;
    }
    rsf_ac7_render_ticket* ticket = nullptr;
    PassLease* lease = nullptr;
    if (valid) {
        lease = new(std::nothrow) PassLease;
        if (lease) {
            lease->owner = self;
            uint64_t table = 0, getter = 0, reference = 0, input = 0; uint32_t output = 0;
            if (read(scope.pass_key, 0, table) && read(table, 8, getter) && getter) {
                reference = uint64_t(uintptr_t(reinterpret_cast<GetPointer>(uintptr_t(getter))(node, 0)));
                if (read(reference, 0, input) && read(reference, 8, output)) lease->input_pool = output_pool(input, output);
            }
            pool_ref(lease->input_pool, 0x28);
            if (index == 3) {
                // All reviewed HUD shader variants bind view+0x13a0's render-target resource.
                uint64_t target = 0, resource = 0, texture = 0;
                if (read(scope.view_key, 0x13a0, target) && read(target, 0x70, resource) &&
                    read(resource, 0x30, texture)) {
                    const auto get_native = method(texture, 0x30);
                    if (get_native) {
                        using Native = void*(*)(void*);
                        lease->ui = static_cast<ID3D11Resource*>(reinterpret_cast<Native>(uintptr_t(get_native))(
                            reinterpret_cast<void*>(uintptr_t(texture))));
                        if (lease->ui) lease->ui->AddRef();
                    }
                }
            }
            const rsf_ac7_scope_lease owned{lease, resolve_pass, release_pass};
            if (!rsf_ac7_render_scope_open_leased(self->scopes, reinterpret_cast<void*>(uintptr_t(list)), &scope, &owned, &ticket)) {
                release_pass(lease); lease = nullptr;
            }
        }
    }
    if (index == 1 && graph_plan && graph_plan->contains(scope.pass_key)) {
        auto* input = input_ref(scope.pass_key); auto* output = node_output(scope.pass_key);
        const auto pool = input ? output_pool(input->node, input->index) : 0;
        if (pool && output) { pool_ref(pool, 0x28); pool_ref(output->pool, 0x30); output->pool = pool; }
        else original(node, context);
    } else original(node, context);
    if (lease) {
        const auto target = output_pool(scope.pass_key, 0);
        pool_ref(target, 0x28);
        lease->output_pool.store(target, std::memory_order_release);
    }
    if (ticket && !rsf_ac7_render_scope_close(ticket, reinterpret_cast<void*>(uintptr_t(list)))) {
        self->active.store(false);
        log(*self, "AC7 renderer deactivated: native command list could not close its pass scope");
    }
}
// Queue-local ownership avoids retaining UObject addresses across garbage collection.
struct WidgetQueue {
    uint64_t converter = 0;
    uint32_t density = 1;
    bool shared = false;
};
thread_local uint64_t initializing_game_instance = 0;
thread_local WidgetQueue widget_queue;
using WidgetPrepare = void(*)(void*);
using WidgetDraw = void(*)(void*, void*, void*, void*, float, uint64_t, float, uint8_t);
// Integer raster density covers output using a 1920x1080 logical canvas; cap to safe texture sizes.
uint32_t widget_density(rsf_ac7_native_renderer* self)
{
    rsf_game_render_config config{}; config.struct_size = sizeof(config);
    if (!self || !self->active.load() || !self->options.render_config ||
        !self->options.render_config(self->options.user, &config) || !config.enabled ||
        !config.output_width || !config.output_height) return 1;
    const auto density = std::max({1u, (config.output_width + 1919u) / 1920u,
        (config.output_height + 1079u) / 1080u});
    return density <= 8 ? density : 1;
}
bool resize_widget_target(uint64_t target, int32_t width, int32_t height)
{
    int32_t extent[2]{};
    if (!read(target, 0xd0, extent) || (extent[0] == width && extent[1] == height)) return false;
    using Init = void(*)(void*, int32_t, int32_t, uint8_t, uint8_t);
    auto* object = reinterpret_cast<void*>(uintptr_t(target));
    reinterpret_cast<Init>(sites[23].original)(object, width, height, 2, 1);
    engine<void(*)(void*, uint8_t)>(0x1ab7dc0)(object, 0);
    return true;
}
// Verify the converter's world/game-instance inheritance before resolving its borrowed shared
// HUD or stereo canvas. An unrelated borrowed target never enters the resize/repaint route.
bool shared_widget_target(uint64_t converter, uint64_t& target)
{
    const auto get_world = method(converter, 0x138);
    if (!get_world) return false;
    const auto world = uint64_t(uintptr_t(reinterpret_cast<void*(*)(void*)>(uintptr_t(get_world))(
        reinterpret_cast<void*>(uintptr_t(converter)))));
    uint64_t instance = 0, actual_class = 0, bases = 0, expected_base = 0;
    int32_t expected_depth = 0, actual_depth = 0;
    const auto expected_class = uint64_t(uintptr_t(engine<void*(*)()>(0x923e60)()));
    if (!read(world, 0x140, instance) || !read(instance, 0x10, actual_class) ||
        !read(expected_class, 0x90, expected_depth) || !read(actual_class, 0x90, actual_depth) ||
        expected_depth < 0 || expected_depth > actual_depth || expected_depth > 4096 ||
        !read(actual_class, 0x88, bases) || !read(bases, size_t(expected_depth) * 8, expected_base) ||
        expected_base != expected_class + 0x88) return false;
    uint8_t stereo = 0;
    return read(converter, 0x79, stereo) && read(instance, stereo ? 0x1f8 : 0x1f0, target) && target;
}
void hooked_game_instance_init(void* instance)
{
    EntryGuard entry;
    const auto previous = initializing_game_instance;
    initializing_game_instance = uint64_t(uintptr_t(instance));
    reinterpret_cast<void(*)(void*)>(sites[22].original)(instance);
    initializing_game_instance = previous;
}
// Promote only the identified game-instance HUD/stereo canvas during its native initialization.
void hooked_target_init(void* target, int32_t width, int32_t height, uint8_t format, uint8_t linear)
{
    EntryGuard entry;
    uint64_t hud = 0, stereo = 0;
    if (initializing_game_instance && width == 1920 && height == 1080 && format == 2 && linear == 1 &&
        read(initializing_game_instance, 0x1f0, hud) && read(initializing_game_instance, 0x1f8, stereo) &&
        (uint64_t(uintptr_t(target)) == hud || uint64_t(uintptr_t(target)) == stereo)) {
        const auto density = widget_density(installed.load(std::memory_order_acquire));
        width *= int32_t(density); height *= int32_t(density);
    }
    using Init = void(*)(void*, int32_t, int32_t, uint8_t, uint8_t);
    reinterpret_cast<Init>(sites[23].original)(target, width, height, format, linear);
}
// Scope converter ownership to this queue call. Resized shared canvases require every contributor
// in this input frame to repaint before native RefreshFPS policy resumes.
void hooked_widget_queue(void* converter, float delta)
{
    EntryGuard entry;
    using Queue = void(*)(void*, float);
    const auto saved = widget_queue;
    widget_queue = {};
    auto* self = installed.load(std::memory_order_acquire);
    uint8_t borrowed = 1;
    const auto address = uint64_t(uintptr_t(converter));
    uint64_t shared = 0;
    if (self && read(address, 0x78, borrowed) && (!borrowed || shared_widget_target(address, shared))) {
        widget_queue = {address, widget_density(self), borrowed != 0};
        if (borrowed) {
            const bool replaced = resize_widget_target(shared, 1920 * int32_t(widget_queue.density),
                1080 * int32_t(widget_queue.density));
            if (replaced && inside_engine_tick) {
                for (auto& canvas : shared_repaint_targets) if (!canvas || canvas == shared) {
                    canvas = shared; break;
                }
            }
            // Every contributor queued in the replacement input frame must repaint the cleared
            // canvas. Resume the engine RefreshFPS policy on subsequent input frames.
            if (replaced || std::find(shared_repaint_targets.begin(), shared_repaint_targets.end(), shared) !=
                shared_repaint_targets.end()) *(static_cast<unsigned char*>(converter) + 0xe0) = 1;
        }
    }
    reinterpret_cast<Queue>(sites[16].original)(converter, delta);
    widget_queue = saved;
}
// Borrow physical DrawSize for target allocation while preserving the game's logical UI layout.
// Resize glow siblings too, because the native producer creates them only on initial allocation.
void hooked_widget_targets(void* converter)
{
    EntryGuard entry;
    const auto original = reinterpret_cast<WidgetPrepare>(sites[17].original);
    const auto address = uint64_t(uintptr_t(converter));
    int32_t logical[2]{};
    if (widget_queue.converter != address || !read(address, 0x28, logical)) {
        original(converter); return;
    }
    // Borrowed targets are game-instance canvases with a verified 1920x1080 logical size.
    // Stock first binding copies their physical extent into DrawSize; restore logical layout.
    if (widget_queue.shared) { logical[0] = 1920; logical[1] = 1080; }
    if (logical[0] <= 0 || logical[1] <= 0 ||
        uint64_t(logical[0]) * widget_queue.density > 16384 ||
        uint64_t(logical[1]) * widget_queue.density > 16384) {
        original(converter); return;
    }
    const int32_t physical[2] = {logical[0] * int32_t(widget_queue.density),
        logical[1] * int32_t(widget_queue.density)};
    uint64_t primary = 0; int32_t previous[2]{};
    const bool resized = !read(address, 0x48, primary) || !read(primary, 0xd0, previous) ||
        previous[0] != physical[0] || previous[1] != physical[1];
    std::memcpy(static_cast<unsigned char*>(converter) + 0x28, physical, sizeof(physical));
    original(converter);
    std::memcpy(static_cast<unsigned char*>(converter) + 0x28, logical, sizeof(logical));
    // A replacement must be painted this queue even when RefreshFPS would skip it.
    if (resized) *(static_cast<unsigned char*>(converter) + 0xe0) = 1;
    // Stock PrepareTargets resizes the primary target, but only creates its glow siblings once.
    using Init = void(*)(void*, int32_t, int32_t, uint8_t, uint8_t);
    using Update = void(*)(void*, uint8_t);
    auto* base = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr));
    uint8_t glow = 0;
    if (!read(address, 0x80, glow) || !glow) return;
    for (const size_t offset : {size_t(0xc0), size_t(0xc8), size_t(0xd0), size_t(0xd8)}) {
        uint64_t target = 0; int32_t extent[2]{};
        const int32_t divisor = offset == 0xd8 ? 1 : 2;
        const int32_t desired[2] = {std::max(1, physical[0] / divisor), std::max(1, physical[1] / divisor)};
        if (!read(address, offset, target) || !read(target, 0xd0, extent) ||
            (extent[0] == desired[0] && extent[1] == desired[1])) continue;
        auto* object = reinterpret_cast<void*>(uintptr_t(target));
        reinterpret_cast<Init>(base + 0x1ab0200)(object, desired[0], desired[1], 2, 1);
        reinterpret_cast<Update>(base + 0x1ab7dc0)(object, 0);
    }
}
// Scale physical raster size and draw scale together only when the owned target matches density;
// logical widget coordinates remain unchanged outside the native draw.
void hooked_widget_draw(void* renderer, void* target, void* hit_grid, void* window,
    float scale, uint64_t packed_size, float delta, uint8_t defer)
{
    EntryGuard entry;
    uint64_t owned = 0; int32_t extent[2]{}; float size[2]{};
    std::memcpy(size, &packed_size, sizeof(size));
    if (widget_queue.converter && read(widget_queue.converter, 0x48, owned) &&
        owned == uint64_t(uintptr_t(target)) && read(owned, 0xd0, extent) &&
        std::isfinite(scale) && scale > 0 && std::isfinite(size[0]) && std::isfinite(size[1]) &&
        size[0] > 0 && size[1] > 0 &&
        extent[0] == int32_t(size[0] * widget_queue.density) &&
        extent[1] == int32_t(size[1] * widget_queue.density)) {
        size[0] *= widget_queue.density; size[1] *= widget_queue.density;
        scale *= widget_queue.density;
        std::memcpy(&packed_size, size, sizeof(size));
    }
    reinterpret_cast<WidgetDraw>(sites[18].original)(renderer, target, hit_grid, window,
        scale, packed_size, delta, defer);
}
// Slate identity follows CPU producer -> queued task -> render execution -> sampled surface.
thread_local WindowSource slate_producer_source{}, slate_execution_source{}, slate_binding_source{};
void hooked_slate_private(void* renderer, void* buffer)
{
    OuterGuard lifetime;
    const auto saved = slate_producer_source; slate_producer_source = {};
    auto* self = installed.load(std::memory_order_acquire);
    uint64_t object = 0, client = 0, viewport = 0, window = 0, control = 0; int32_t refs = 0;
    if (self && self->active.load() && inside_engine_tick && input_source_frame &&
        copy(&object, reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + 0x3cbbc28, 8) &&
        read(object, 0x720, client) && read(client, 0xa0, viewport) && viewport &&
        read(object, 0xdf8, window) && window && read(object, 0xe00, control) &&
        read(control, 8, refs) && refs > 0) {
        slate_producer_source.source = input_source_frame; slate_producer_source.viewport = viewport;
        slate_producer_source.window = window; slate_producer_source.renderer = uint64_t(uintptr_t(renderer));
        slate_producer_source.after_simulation = after_simulation;
    }
    reinterpret_cast<void(*)(void*, void*)>(sites[28].original)(renderer, buffer);
    slate_producer_source = saved;
}
// Bind the allocated task until its execution consumes the entry. Duplicate address/full-table
// refusal preserves occupied records without associating future work with a stale source.
void* hooked_slate_allocate(void* result, uint64_t prerequisite, uint32_t priority)
{
    EntryGuard producer;
    auto* returned = reinterpret_cast<void*(*)(void*, uint64_t, uint32_t)>(sites[29].original)(result, prerequisite, priority);
    auto* self = installed.load(std::memory_order_acquire);
    uint64_t task = 0;
    if (self && self->active.load() && slate_producer_source.source &&
        read(uint64_t(uintptr_t(returned)), 0, task) && task) {
        bool refused = true;
        {
            std::lock_guard<std::mutex> lock(self->identities_guard);
            bool duplicate = false;
            for (auto& slot : self->window_tasks) if (slot.task == task) {
                // A missed task retirement must not associate a reused address with its old
                // source frame. Preserve the occupied entry but make its identity unusable.
                slot.source = 0; duplicate = true; break;
            }
            if (!duplicate) for (auto& slot : self->window_tasks) if (!slot.task) {
                slot = slate_producer_source; slot.task = task; ++self->live_window_tasks;
                refused = false; break;
            }
        }
        if (refused) log(*self, "AC7 window source refused: duplicate or full live task table; no binding evicted");
    }
    return returned;
}
void hooked_slate_task(void* task)
{
    OuterGuard lifetime;
    const auto saved = slate_execution_source; slate_execution_source = {};
    auto* self = installed.load(std::memory_order_acquire);
    WindowSource source{};
    if (self) {
        EntryGuard producer;
        std::lock_guard<std::mutex> lock(self->identities_guard);
        for (auto& slot : self->window_tasks) if (slot.task == uint64_t(uintptr_t(task))) {
            source = slot; slot = {}; --self->live_window_tasks; break;
        }
    }
    uint64_t window = 0;
    if (source.source && read(source.task, 0x28, window) && window == source.window &&
        read(source.task, 0x10, source.renderer) && read(source.task, 0x18, source.info) &&
        read(source.task, 0x20, source.elements)) slate_execution_source = source;
    reinterpret_cast<void(*)(void*)>(sites[30].original)(task);
    slate_execution_source = saved;
}
// Retain concrete swap-chain identity through queued Slate-window execution, not a texture alias.
struct WindowLease { IDXGISwapChain* swapchain = nullptr; };
void resolve_window(void* object, rsf_ac7_render_scope* scope)
{ scope->swapchain = static_cast<WindowLease*>(object)->swapchain; }
void release_window(void* object)
{
    auto* lease = static_cast<WindowLease*>(object);
    if (lease->swapchain) lease->swapchain->Release();
    delete lease;
}
// Accept the exact task/window/elements association, including the synchronous producer path,
// then retain its concrete swap chain through queued native window work.
void hooked_slate_window(void* renderer, void* list, void* info, void* elements, uint8_t vsync, uint8_t clear)
{
    OuterGuard lifetime;
    auto* self = installed.load(std::memory_order_acquire);
    using Draw = void(*)(void*, void*, void*, void*, uint8_t, uint8_t);
    auto original = reinterpret_cast<Draw>(sites[31].original);
    auto source = slate_execution_source;
    const auto element_key = uint64_t(uintptr_t(elements));
    if (!source.source && slate_producer_source.source) {
        uint64_t window = 0; read(element_key, 0x10, window);
        if (!window) read(element_key, 0, window);
        if (window == slate_producer_source.window) {
            source = slate_producer_source; source.info = uint64_t(uintptr_t(info)); source.elements = element_key;
        }
    }
    rsf_ac7_render_ticket* ticket = nullptr;
    if (self && self->active.load() && source.source && source.window && source.renderer == uint64_t(uintptr_t(renderer)) &&
        source.info == uint64_t(uintptr_t(info)) && source.elements == element_key) {
        uint64_t rhi = 0;
        if (read(source.info, 0x70, rhi)) {
            // FRHIViewport's first method after its destructor is GetNativeSwapChain. The
            // matched DX11 viewport override returns IDXGISwapChain, not a texture resource.
            const auto getter = method(rhi, 8);
            using Native = void*(*)(void*);
            auto* native = getter ? static_cast<IUnknown*>(reinterpret_cast<Native>(uintptr_t(getter))(
                reinterpret_cast<void*>(uintptr_t(rhi)))) : nullptr;
            auto* lease = new(std::nothrow) WindowLease;
            if (lease && native && SUCCEEDED(native->QueryInterface(__uuidof(IDXGISwapChain),
                    reinterpret_cast<void**>(&lease->swapchain)))) {
                rsf_ac7_render_scope scope{}; scope.struct_size = sizeof(scope);
                scope.session_id = self->options.session_id; scope.role = RSF_GAME_RENDER_WINDOW;
                scope.source_frame_id = source.source; scope.viewport_key = source.viewport;
                scope.window_key = source.window; scope.rhi_viewport_key = rhi;
                scope.pass_key = source.task ? source.task : element_key; scope.flags = RSF_GAME_RENDER_PRIMARY;
                if (source.after_simulation) scope.flags |= RSF_GAME_RENDER_AFTER_SIMULATION;
                const rsf_ac7_scope_lease owned{lease, resolve_window, release_window};
                if (!rsf_ac7_render_scope_open_leased(self->scopes, list, &scope, &owned, &ticket)) {
                    release_window(lease);
                    static std::atomic<uint32_t> refusals{0};
                    if (refusals.fetch_add(1) < 4) log(*self, "AC7 window scope refused: source Present ownership was not queued");
                }
            } else if (lease) release_window(lease);
        }
    }
    const auto saved_binding = slate_binding_source;
    slate_binding_source = ticket ? source : WindowSource{};
    original(renderer, list, info, elements, vsync, clear);
    slate_binding_source = saved_binding;
    if (ticket && !rsf_ac7_render_scope_close(ticket, list)) {
        self->active.store(false); log(*self, "AC7 window source deactivated: native RHI scope could not close");
    }
}
// Exact final-scene surface sampled by the admitted window, retained through binding execution.
struct TextureBindingLease { ID3D11Resource* texture = nullptr; };
void resolve_texture_binding(void* object, rsf_ac7_render_scope* scope)
{ scope->sampled_texture = static_cast<TextureBindingLease*>(object)->texture; }
void release_texture_binding(void* object)
{
    auto* lease = static_cast<TextureBindingLease*>(object);
    if (lease->texture) lease->texture->Release();
    delete lease;
}
// Emit a validation scope only for this source's unambiguous final scene surface. Ordinary fonts
// and icons forward directly and acquire no diagnostic binding lease.
void hooked_slate_texture(void* shader, void* list, void* rhi_texture, void* sampler_ref)
{
    EntryGuard producer;
    auto* self = installed.load(std::memory_order_acquire);
    const auto source = slate_binding_source;
    rsf_ac7_render_ticket* ticket = nullptr;
    uint16_t count = 0, slot = 0;
    if (self && self->active.load() && source.source && source.window &&
        read(uint64_t(uintptr_t(shader)), 0xc2, count) && count &&
        read(uint64_t(uintptr_t(shader)), 0xc0, slot)) {
        auto* texture = static_cast<ID3D11Resource*>(native_texture(uint64_t(uintptr_t(rhi_texture))));
        bool candidate = false;
        {
            std::lock_guard<std::mutex> lock(self->identities_guard);
            const auto& key = self->final_surfaces[source.source % self->final_surfaces.size()];
            candidate = key.source == source.source && key.viewport == source.viewport && !key.ambiguous &&
                key.surface == uint64_t(uintptr_t(texture)) && texture;
        }
        // Most Slate batches are fonts/icons. Only a surface produced by this exact scene source
        // needs a queued validation marker and resource lease.
        auto* lease = candidate ? new(std::nothrow) TextureBindingLease : nullptr;
        if (lease) {
            lease->texture = texture;
            if (lease->texture) {
                lease->texture->AddRef();
                rsf_ac7_render_scope scope{}; scope.struct_size = sizeof(scope);
                scope.session_id = self->options.session_id; scope.role = RSF_GAME_RENDER_TEXTURE_BINDING;
                scope.source_frame_id = source.source; scope.viewport_key = source.viewport;
                scope.window_key = source.window; scope.pass_key = uint64_t(uintptr_t(shader));
                scope.texture_slot = slot; scope.flags = RSF_GAME_RENDER_PRIMARY;
                if (source.after_simulation) scope.flags |= RSF_GAME_RENDER_AFTER_SIMULATION;
                const rsf_ac7_scope_lease owned{lease, resolve_texture_binding, release_texture_binding};
                if (!rsf_ac7_render_scope_open_leased(self->scopes, list, &scope, &owned, &ticket)) {
                    release_texture_binding(lease);
                    static std::atomic<uint32_t> refusals{0};
                    if (refusals.fetch_add(1) < 4) log(*self, "AC7 texture-binding scope refused: final scene/window association was not queued");
                }
            } else delete lease;
        }
    }
    reinterpret_cast<void(*)(void*, void*, void*, void*)>(sites[32].original)(shader, list, rhi_texture, sampler_ref);
    if (ticket && !rsf_ac7_render_scope_close(ticket, list)) {
        self->active.store(false); log(*self, "AC7 texture binding deactivated: native RHI scope could not close");
    }
}
thread_local bool sky_projection_jitter = false;
constexpr unsigned char native_cloud_divisor[]{0x8b,0x87,0x10,0x02,0x00,0x00};
constexpr unsigned char full_cloud_divisor[]{0xb8,0x01,0x00,0x00,0x00,0x90};
constexpr unsigned char native_cloud_depth_format[]{0x16,0x00,0x00,0x00};
constexpr unsigned char precise_cloud_depth_format[]{0x05,0x00,0x00,0x00};
constexpr unsigned char native_scene_format[]{0x48,0x8b,0x05,0x29,0x33,0xbc,0x02,0x48,0x8b,0xcb,0x8b,0x40,0x04};
constexpr unsigned char precise_scene_format[]{0xb8,0x04,0x00,0x00,0x00,0x90,0x90,0x48,0x8b,0xcb,0x90,0x90,0x90};
// Compare exact current bytes before either patching or restoring code, then flush the instruction
// cache. A foreign patch/build mismatch refuses and leaves controller ownership with the caller.
bool write_render_code(unsigned char* site, const unsigned char* expected, const unsigned char* replacement, size_t size)
{
    unsigned char actual[16]{};
    if (!size || size > sizeof(actual) || !copy(actual, site, size) || std::memcmp(actual, expected, size)) return false;
    DWORD protection = 0;
    if (!VirtualProtect(site, size, PAGE_EXECUTE_READWRITE, &protection)) return false;
    std::memcpy(site, replacement, size);
    FlushInstructionCache(GetCurrentProcess(), site, size);
    DWORD ignored = 0; VirtualProtect(site, size, protection, &ignored);
    return true;
}
// Replace only the guarded native depth dispatch after actual b11/t1/u0 format/extent validation.
// Binding changes revoke full-grid readiness; shader/device ownership survives until safe stop.
void STDMETHODCALLTYPE native_cloud_depth_dispatch(ID3D11DeviceContext* context, UINT x, UINT y, UINT z)
{
    OuterGuard outer; EntryGuard entry;
    using Dispatch = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*,UINT,UINT,UINT);
    auto forward = reinterpret_cast<Dispatch>((*reinterpret_cast<void***>(context))[41]);
    auto* self = installed.load(std::memory_order_acquire);
    if (!self) { forward(context,x,y,z); return; }
    Microsoft::WRL::ComPtr<ID3D11Device> device; context->GetDevice(&device);
    {
        std::lock_guard<std::mutex> lock(self->cloud_resolution_guard);
        // A recreated D3D device cannot use the previous device's shader. Native texture ownership
        // supplies the replacement views; rebuild only our shader before validating this dispatch.
        if (self->cloud_depth_device && self->cloud_depth_device != device.Get()) {
            self->cloud_depth_shader->Release(); self->cloud_depth_shader = nullptr;
            self->cloud_depth_device->Release(); self->cloud_depth_device = nullptr;
            self->cloud_depth_shader_refused = false;
            self->cloud_depth_ready.store(false,std::memory_order_release);
        }
        if (!self->cloud_depth_shader && !self->cloud_depth_shader_refused) {
            if (rsf_ac7_create_truesky_depth_shader(device.Get(),&self->cloud_depth_shader)) {
                self->cloud_depth_device = device.Get(); self->cloud_depth_device->AddRef();
                log(*self,"native TrueSky: one-to-one depth shader compiled; checking native pass bindings");
            } else {
                self->cloud_depth_shader_refused = true;
                log(*self,"native TrueSky one-to-one depth shader refused; retaining supported native cloud resolution");
            }
        }
    }
    // This call site belongs only to native screenspace depth production. Identify its active
    // resolution from the actual resources, including even padding, instead of guessing a view.
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> source;
    Microsoft::WRL::ComPtr<ID3D11UnorderedAccessView> target;
    Microsoft::WRL::ComPtr<ID3D11Buffer> constants;
    context->CSGetShaderResources(1,1,&source); context->CSGetUnorderedAccessViews(0,1,&target);
    context->CSGetConstantBuffers(11,1,&constants);
    bool bindings = self->cloud_depth_shader && self->cloud_depth_device == device.Get() && source && target && constants;
    bool full = false;
    D3D11_TEXTURE2D_DESC src{},dst{}; D3D11_BUFFER_DESC cb{};
    D3D11_SHADER_RESOURCE_VIEW_DESC srv{}; D3D11_UNORDERED_ACCESS_VIEW_DESC uav{};
    if (bindings) {
        Microsoft::WRL::ComPtr<ID3D11Resource> src_resource, dst_resource;
        Microsoft::WRL::ComPtr<ID3D11Texture2D> src_texture, dst_texture;
        source->GetResource(&src_resource); target->GetResource(&dst_resource);
        bindings = SUCCEEDED(src_resource.As(&src_texture)) && SUCCEEDED(dst_resource.As(&dst_texture));
        if (bindings) {
            src_texture->GetDesc(&src); dst_texture->GetDesc(&dst); constants->GetDesc(&cb);
            source->GetDesc(&srv); target->GetDesc(&uav);
            bindings = src.SampleDesc.Count == 1 && src.Width > 0 && src.Height > 0 && cb.ByteWidth == 352 &&
                srv.ViewDimension == D3D11_SRV_DIMENSION_TEXTURE2D &&
                srv.Texture2D.MostDetailedMip == 0 && dst.SampleDesc.Count == 1 && dst.ArraySize == 1 &&
                uav.ViewDimension == D3D11_UAV_DIMENSION_TEXTURE2D && uav.Texture2D.MipSlice == 0 &&
                dst.Format == DXGI_FORMAT_R32G32_FLOAT && uav.Format == DXGI_FORMAT_R32G32_FLOAT;
            full = bindings && dst.Width <= src.Width+2 && dst.Width+2 >= src.Width &&
                dst.Height <= src.Height+2 && dst.Height+2 >= src.Height;
        }
    }
    Microsoft::WRL::ComPtr<ID3D11ComputeShader> original;
    UINT classes = 0;
    if (bindings) {
        context->CSGetShader(&original,nullptr,&classes);
        bindings = original && classes == 0;
        full = full && bindings;
    }
    if (bindings && !self->cloud_depth_ready.exchange(true,std::memory_order_acq_rel)) {
        char message[192]{};
        std::snprintf(message,sizeof(message),"native TrueSky: b11/t1/u0 Texture2D RG32F depth bindings verified (%ux%u -> %ux%u); full cloud grid may activate",
            src.Width,src.Height,dst.Width,dst.Height);
        log(*self,message);
    }
    if (!bindings && self->cloud_depth_binding_refusals.fetch_add(1) < 4) {
        char message[320]{};
        std::snprintf(message,sizeof(message),"native TrueSky depth binding refused: srv=%u uav=%u cb=%u, SRVdim=%u UAVdim=%u targetFormat=%u viewFormat=%u, source=%ux%u target=%ux%u classes=%u",
            source ? 1u : 0u,target ? 1u : 0u,cb.ByteWidth,unsigned(srv.ViewDimension),unsigned(uav.ViewDimension),
            unsigned(dst.Format),unsigned(uav.Format),src.Width,src.Height,dst.Width,dst.Height,classes);
        log(*self,message);
    }
    if (!bindings && self->cloud_resolution_site) {
        // Readiness of an earlier frame does not authorize running x2 on this frame's x1 targets.
        // Retire full-grid mode for the next native allocation and reject this unverified dispatch.
        // The enclosing native function still performs its balanced Unapply/cleanup.
        std::lock_guard<std::mutex> lock(self->cloud_resolution_guard);
        self->cloud_depth_ready.store(false,std::memory_order_release);
        self->cloud_resolution_refused = true;
        if (write_render_code(self->cloud_resolution_site,full_cloud_divisor,native_cloud_divisor,sizeof(native_cloud_divisor))) {
            self->cloud_resolution_site = nullptr;
            log(*self,"native TrueSky: full-grid depth bindings changed; unsafe dispatch refused, supported cloud grid restored for next native frame");
        } else {
            log(*self,"native TrueSky: full-grid depth bindings changed and divisor restore failed; unsafe dispatch refused, renderer retained");
        }
        return;
    }
    if (full) {
        context->CSSetShader(self->cloud_depth_shader,nullptr,0);
        if (self->cloud_depth_dispatches.fetch_add(1) < 3)
            log(*self,"native TrueSky: normalized one-to-one scene depth dispatched on native effect resources");
    }
    forward(context,x,y,z);
    // Restore the exact pass shader before TrueSky's own Unapply, keeping its cached state valid.
    if (full) context->CSSetShader(original.Get(),nullptr,0);
}
// Allocate nearby indirection storage within signed rel32 reach of a guarded native call site.
void* allocate_depth_relay(unsigned char* call)
{
    SYSTEM_INFO info{}; GetSystemInfo(&info);
    const auto granularity = uintptr_t(info.dwAllocationGranularity);
    const auto centre = uintptr_t(call) & ~(granularity-1);
    for (uintptr_t delta=granularity; delta < 0x70000000; delta+=granularity) {
        for (const auto candidate : {centre+delta, centre>delta ? centre-delta : uintptr_t(0)}) {
            if (!candidate) continue;
            MEMORY_BASIC_INFORMATION region{};
            if (VirtualQuery(reinterpret_cast<void*>(candidate),&region,sizeof(region)) && region.State == MEM_FREE) {
                auto* memory = VirtualAlloc(reinterpret_cast<void*>(candidate),4096,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE);
                if (memory) return memory;
            }
        }
    }
    return nullptr;
}
// Pair executable bytes with the shipped TrueSky effect size/CRC before replacing its depth route.
bool depth_effect_fingerprint()
{
    wchar_t file[32768]{};
    if (!GetModuleFileNameW(nullptr,file,32768)) return false;
    auto* slash = std::wcsrchr(file,L'\\'); if (!slash) return false;
    *slash = 0;
    constexpr wchar_t suffix[] = L"\\Engine\\Plugins\\TrueSkyPlugin\\shaderbin\\Win64\\mixed_resolution.fxo";
    if (std::wcslen(file)+std::wcslen(suffix) >= 32768) return false;
    std::wcscat(file,suffix);
    auto* input = _wfopen(file,L"rb"); if (!input) return false;
    uint32_t crc=0xffffffff, size=0; unsigned char bytes[4096]{};
    size_t count=0;
    while ((count=std::fread(bytes,1,sizeof(bytes),input)) != 0 && size < 1048576) {
        size += uint32_t(count);
        for (size_t i=0;i<count;++i) {
            crc ^= bytes[i];
            for (uint32_t bit=0;bit<8;++bit) crc=(crc>>1) ^ (0xedb88320u & (0u-(crc&1u)));
        }
    }
    const bool read_ok = std::feof(input) && !std::ferror(input); std::fclose(input);
    return read_ok && size == 45263 && ~crc == 0x78881ef3;
}
// Install a reversible native-call relay and matching depth-format/branch patches. Full cloud
// resolution stays disabled until the actual GPU pass verifies shader compilation and bindings.
bool install_depth_dispatch(rsf_ac7_native_renderer& self, unsigned char* module)
{
    if (self.cloud_depth_call) return self.cloud_depth_format_site != nullptr;
    auto* call = module+0xc003f; auto* branch=module+0xbff0b;
    auto* format = module+0xbfc05;
    constexpr unsigned char call_expected[]{0x41,0xff,0x92,0x48,0x01,0x00,0x00,0x48,0x8b,0x4b,0x08,0x4c,0x8d,0x83,0xb0,0x01};
    constexpr unsigned char branch_expected[]{0x41,0x83,0xfc,0x02,0x75,0x1d,0x48,0x8b,0x4b,0x08,0x4c,0x8d,0x43,0x18,0x48,0x8b};
    constexpr unsigned char format_expected[]{0xc7,0x44,0x24,0x20,0x16,0x00,0x00,0x00,0xff,0x50,0x58,0x8b,0x87,0x94,0x00,0x00};
    unsigned char actual[16]{};
    if (!copy(actual,call,16) || std::memcmp(actual,call_expected,16) ||
        !copy(actual,branch-4,16) || std::memcmp(actual,branch_expected,16) ||
        !copy(actual,format,16) || std::memcmp(actual,format_expected,16) || !depth_effect_fingerprint()) return false;
    auto* relay = allocate_depth_relay(call); if (!relay) return false;
    const auto handler = reinterpret_cast<void*>(&native_cloud_depth_dispatch);
    std::memcpy(relay,&handler,sizeof(handler));
    unsigned char replacement[]{0xff,0x15,0,0,0,0,0x90};
    const auto distance = intptr_t(relay)-intptr_t(call+6);
    if (distance < INT32_MIN || distance > INT32_MAX) { VirtualFree(relay,0,MEM_RELEASE); return false; }
    const int32_t relative = int32_t(distance); std::memcpy(replacement+2,&relative,4);
    constexpr unsigned char old_branch[]{0x75}, supported_branch[]{0x77};
    if (!write_render_code(branch,old_branch,supported_branch,1)) { VirtualFree(relay,0,MEM_RELEASE); return false; }
    if (!write_render_code(call,call_expected,replacement,sizeof(replacement))) {
        write_render_code(branch,supported_branch,old_branch,1); VirtualFree(relay,0,MEM_RELEASE); return false;
    }
    self.cloud_depth_call=call; self.cloud_depth_branch=branch; self.cloud_depth_relay=relay;
    std::memcpy(self.cloud_depth_call_bytes,replacement,sizeof(replacement));
    // This argument belongs to normalized scene-depth production, not cloud colour or scene Z.
    // The native ensure2D owner recreates the texture and both views when its format changes.
    if (!write_render_code(format+4,native_cloud_depth_format,precise_cloud_depth_format,sizeof(native_cloud_depth_format))) {
        log(self,"native TrueSky precise depth allocation refused at DLL RVA 0xbfc09; full cloud grid remains disabled");
        return false;
    }
    self.cloud_depth_format_site=format+4;
    log(self,"native TrueSky: normalized scene-depth bounds use R32G32_FLOAT at DLL RVA 0xbfc09; native owner rebuilds matching texture/views");
    log(self,"native TrueSky depth: guarded one-to-one pass route installed at DLL RVAs 0xbff0b/0xc003f; waiting for GPU shader preparation");
    return true;
}
// composite_tile reaches TrueSky's render platform at DLL RVA 0xabc6b, `call [rax+0x130]` with
// (platform, deviceContext&, count). The relay forwards it, then reads the pass's still-bound
// cloud inputs before TrueSky's Unapply and hands the host a device depth for cloud motion.
void native_cloud_composite_draw(void* platform, void* device_context, uint32_t count)
{
    OuterGuard outer; EntryGuard entry;
    using PlatformDraw = void(*)(void*, void*, uint32_t);
    reinterpret_cast<PlatformDraw>((*static_cast<void***>(platform))[0x130 / 8])(platform, device_context, count);
    auto* self = installed.load(std::memory_order_acquire);
    if (!self || !self->active.load() || !self->options.on_stage) return;
    Microsoft::WRL::ComPtr<ID3D11DeviceContext> context;
    {
        std::lock_guard<std::mutex> lock(self->cloud_resolution_guard);
        if (!self->cloud_depth_device) return; // Known only after TrueSky's depth producer ran.
        self->cloud_depth_device->GetImmediateContext(&context);
    }
    D3D11_VIEWPORT viewport{}; UINT viewports = 1; context->RSGetViewports(&viewports, &viewport);
    auto* depth = viewports == 1 ? rsf_ac7_cloud_depth_write(self->cloud_motion, context.Get(), viewport) : nullptr;
    const auto reports = self->cloud_motion_reports.fetch_add(1);
    if (!depth) {
        if (reports < 3) log(*self, "native TrueSky cloud motion refused: composite_tile bindings differ from the measured t1/t2/b12 layout");
        return;
    }
    rsf_ac7_render_scope pass{}; pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_CLOUD_DEPTH;
    pass.session_id = self->options.session_id; pass.depth = depth;
    pass.render_rect[0] = int32_t(viewport.TopLeftX); pass.render_rect[1] = int32_t(viewport.TopLeftY);
    pass.render_rect[2] = pass.render_rect[0] + int32_t(viewport.Width); pass.render_rect[3] = pass.render_rect[1] + int32_t(viewport.Height);
    self->options.on_stage(self->options.user, nullptr, &pass, 1);
    if (reports < 3) log(*self, "native TrueSky cloud motion: composite_tile cloud distance converted to scene device depth");
}
void install_cloud_motion(rsf_ac7_native_renderer& self, unsigned char* module)
{
    if (self.cloud_motion_call) return;
    auto* call = module + 0xabc6b;
    constexpr unsigned char before[]{0x49,0x8b,0x8e,0x28,0x02,0x00,0x00,0x45,0x8b,0xc4,0x49,0x8b,0xd7,0x48,0x8b,0x01};
    constexpr unsigned char expected[]{0xff,0x90,0x30,0x01,0x00,0x00,0x49,0x8b,0x9e,0x60,0x02,0x00,0x00,0x48,0x8d,0x0d};
    unsigned char actual[16]{};
    if (!copy(actual, call - 16, 16) || std::memcmp(actual, before, 16) || !copy(actual, call, 16) || std::memcmp(actual, expected, 16)) {
        log(self, "native TrueSky cloud motion refused: composite_tile draw bytes differ at DLL RVA 0xabc6b"); return;
    }
    auto* relay = allocate_depth_relay(call); if (!relay) return;
    const auto handler = reinterpret_cast<void*>(&native_cloud_composite_draw);
    std::memcpy(relay, &handler, sizeof(handler));
    unsigned char replacement[]{0xff,0x15,0,0,0,0};
    const auto distance = intptr_t(relay) - intptr_t(call + 6);
    if (distance < INT32_MIN || distance > INT32_MAX) { VirtualFree(relay, 0, MEM_RELEASE); return; }
    const int32_t relative = int32_t(distance); std::memcpy(replacement + 2, &relative, 4);
    if (!write_render_code(call, expected, replacement, sizeof(replacement))) { VirtualFree(relay, 0, MEM_RELEASE); return; }
    self.cloud_motion_call = call; self.cloud_motion_relay = relay;
    log(self, "native TrueSky cloud motion: composite_tile draw relay installed at DLL RVA 0xabc6b");
}
// Select scene allocation precision at the original policy producer so the native cache rebuilds
// matching engine-owned resources, rather than widening already-quantized rendered values.
bool apply_scene_precision_patch(rsf_ac7_native_renderer& self)
{
    if (self.scene_precision_site) return true;
    auto* site = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + 0x1095620;
    constexpr unsigned char expected[]{0x48,0x8b,0x05,0x29,0x33,0xbc,0x02,0x48,0x8b,0xcb,0x8b,0x40,0x04,0x89,0x45,0xd7};
    unsigned char actual[sizeof(expected)]{};
    if (!copy(actual, site, sizeof(actual)) || std::memcmp(actual, expected, sizeof(actual)) ||
        !write_render_code(site, native_scene_format, precise_scene_format, sizeof(native_scene_format))) {
        log(self, "native scene-precision patch refused: expected bytes differ or code protection failed at RVA 0x1095620");
        return false;
    }
    // Allocate compares this effective format to its cache before UpdateRHI. Existing packed
    // targets therefore retire through the engine's own reallocation path on the next family.
    self.scene_precision_site = site;
    log(self, "native scene precision: allocation format policy 4 at RVA 0x1095620; engine owns FP16 resource rebuild");
    return true;
}
// Called before native sky rendering, serialized with GPU shader preparation. Activate the full
// grid only after both code/effect fingerprinting and actual-device binding checks establish readiness.
void apply_cloud_resolution_patch(rsf_ac7_native_renderer& self)
{
    std::lock_guard<std::mutex> lock(self.cloud_resolution_guard);
    if (self.cloud_resolution_site || self.cloud_resolution_refused) return;
    auto* module = reinterpret_cast<unsigned char*>(GetModuleHandleW(L"TrueSkyPluginRender_MT.dll"));
    if (!module) return;
    install_cloud_motion(self, module);
    if (!install_depth_dispatch(self,module)) {
        self.cloud_resolution_refused=true;
        log(self,"native TrueSky full-resolution patch refused: depth producer code/effect fingerprint or relay allocation differs");
        return;
    }
    // Run the first depth pass at its supported native divisor. Enable the full grid only after
    // the replacement kernel has compiled on the actual device; failure keeps stock rendering.
    if (!self.cloud_depth_ready.load(std::memory_order_acquire)) return;
    // RenderInterfaceViewFrame passes q.downscale into the native per-view allocator. Patching
    // this consumer also covers settings the actor or a sequence writes after startup.
    constexpr unsigned char expected[]{0x8b,0x87,0x10,0x02,0x00,0x00,0x89,0x44,0x24,0x38,0x48,0x8b,0x45,0xd0,0x0f,0x29};
    auto* site = module + 0x87f6d;
    unsigned char actual[sizeof(expected)]{};
    if (!copy(actual, site, sizeof(actual)) || std::memcmp(actual, expected, sizeof(actual)) ||
        !write_render_code(site, native_cloud_divisor, full_cloud_divisor, sizeof(native_cloud_divisor))) {
        self.cloud_resolution_refused = true;
        log(self, "native TrueSky cloud-resolution patch refused: expected bytes differ or code protection failed at DLL RVA 0x87f6d");
        return;
    }
    self.cloud_resolution_site = site;
    log(self, "native TrueSky: cloud/depth producer uses scene resolution, divisor 1 at DLL RVA 0x87f6d");
}
void hooked_sky_projection(float* matrix, float units)
{
    EntryGuard entry;
    reinterpret_cast<void(*)(float*, float)>(sites[44].original)(matrix, units);
    // The native handedness conversion negates M20 and M23. M21 must follow M20
    // for temporal projection offsets to retain the scene's vertical sampling direction.
    if (sky_projection_jitter && matrix) matrix[9] = -matrix[9];
}
void hooked_sky_render(void* plugin, void* parameters)
{
    OuterGuard outer; EntryGuard entry;
    const bool previous = sky_projection_jitter;
    sky_projection_jitter = false;
    auto* self = installed.load();
    rsf_game_render_config config{}; config.struct_size = sizeof(config);
    uint64_t view = 0, cached = 0; float jitter_y = 0, projection_y = 0, perspective = 0;
    const auto key = uint64_t(uintptr_t(parameters));
    if (self && self->active.load()) apply_cloud_resolution_patch(*self);
    if (self && self->active.load() && self->options.render_config &&
        self->options.render_config(self->options.user, &config) && config.enabled &&
        read(key, 0xb0, view) && primary_view(view, config) &&
        read(view, 0x1418, cached) && read(cached, 0x724, jitter_y) &&
        read(key, 0x74, projection_y) && read(key, 0x7c, perspective) &&
        std::isfinite(jitter_y) && jitter_y != 0 && std::fabs(projection_y - jitter_y) < 1e-6f &&
        std::fabs(perspective - 1.0f) < 1e-6f) {
        sky_projection_jitter = true;
        static uint32_t reports = 0;
        if (reports++ < 4) log(*self, "native TrueSky: matched primary-view vertical jitter handedness corrected");
    }
    reinterpret_cast<Process>(sites[45].original)(plugin, parameters);
    sky_projection_jitter = previous;
}
uint32_t hooked_scene_colour_format(void* scene)
{
    EntryGuard entry;
    const auto original = reinterpret_cast<uint32_t(*)(void*)>(sites[47].original);
    auto* self = installed.load();
    int32_t feature = 0;
    if (!self || !self->active.load() || !read(uint64_t(uintptr_t(scene)), 0x258, feature) || feature < 2)
        return original(scene);
    // Preserve precision where lighting and sky first write colour. Widening an R11 texture
    // after rendering cannot recover its discarded mantissa bits.
    static std::atomic<bool> reported{false};
    if (!reported.exchange(true)) {
        log(*self, "native scene colour: PF_FloatRGBA selected at allocation and shader-parameter producer, RVA 0x109f300");
    }
    return 10;
}
template<uint32_t I> void hooked_process(void* node, void* context) { process(I, node, context); }
void* hooks[] = {reinterpret_cast<void*>(&hooked_process<0>), reinterpret_cast<void*>(&hooked_process<1>),
    reinterpret_cast<void*>(&hooked_process<2>), reinterpret_cast<void*>(&hooked_process<3>),
    reinterpret_cast<void*>(&hooked_process<4>), reinterpret_cast<void*>(&hooked_process<5>),
    reinterpret_cast<void*>(&hooked_descriptor<0>), reinterpret_cast<void*>(&hooked_descriptor<1>),
    reinterpret_cast<void*>(&hooked_descriptor<2>), reinterpret_cast<void*>(&hooked_descriptor<3>),
    reinterpret_cast<void*>(&hooked_descriptor<4>), reinterpret_cast<void*>(&hooked_descriptor<5>),
    reinterpret_cast<void*>(&hooked_postprocess), reinterpret_cast<void*>(&hooked_context),
    reinterpret_cast<void*>(&hooked_visibility), reinterpret_cast<void*>(&hooked_construct), reinterpret_cast<void*>(&hooked_widget_queue),
    reinterpret_cast<void*>(&hooked_widget_targets), reinterpret_cast<void*>(&hooked_widget_draw),
    reinterpret_cast<void*>(&hooked_engine_tick), reinterpret_cast<void*>(&hooked_renderer_retire),
    reinterpret_cast<void*>(&hooked_poll_input), reinterpret_cast<void*>(&hooked_game_instance_init),
    reinterpret_cast<void*>(&hooked_target_init), reinterpret_cast<void*>(&hooked_simulation<24>),
    reinterpret_cast<void*>(&hooked_simulation<25>), reinterpret_cast<void*>(&hooked_redraw),
    reinterpret_cast<void*>(&hooked_viewport_draw), reinterpret_cast<void*>(&hooked_slate_private),
    reinterpret_cast<void*>(&hooked_slate_allocate), reinterpret_cast<void*>(&hooked_slate_task),
    reinterpret_cast<void*>(&hooked_slate_window), reinterpret_cast<void*>(&hooked_slate_texture),
    reinterpret_cast<void*>(&hooked_translucency_size), reinterpret_cast<void*>(&hooked_unmodified_begin),
    reinterpret_cast<void*>(&hooked_unmodified_resolve), reinterpret_cast<void*>(&hooked_translucency_render),
    reinterpret_cast<void*>(&hooked_pixel_view_uniform), reinterpret_cast<void*>(&hooked_rhi_pixel_uniform),
    reinterpret_cast<void*>(&hooked_rhi_pixel_tables), reinterpret_cast<void*>(&hooked_pixel_enqueue<40>),
    reinterpret_cast<void*>(&hooked_pixel_enqueue<41>), reinterpret_cast<void*>(&hooked_pixel_enqueue<42>),
    reinterpret_cast<void*>(&hooked_pixel_enqueue<43>), reinterpret_cast<void*>(&hooked_sky_projection),
    reinterpret_cast<void*>(&hooked_sky_render), reinterpret_cast<void*>(&hooked_temporal_sample_index),
    reinterpret_cast<void*>(&hooked_scene_colour_format), reinterpret_cast<void*>(&hooked_render_family),
    reinterpret_cast<void*>(&hooked_frame_task_construct), reinterpret_cast<void*>(&hooked_frame_task_execute),
    reinterpret_cast<void*>(&hooked_rhi_frame_begin), reinterpret_cast<void*>(&hooked_rhi_frame_end),
    reinterpret_cast<void*>(&hooked_frame_sync), reinterpret_cast<void*>(&hooked_pump_messages),
    reinterpret_cast<void*>(&hooked_engine_pacing), reinterpret_cast<void*>(&hooked_create_pixel_shader),
    reinterpret_cast<void*>(&hooked_base_pass)};
}
extern "C" int rsf_ac7_native_renderer_prepare(const rsf_ac7_native_renderer_options* options,
    rsf_ac7_native_renderer** out) try
{
    if (!options || options->struct_size < sizeof(*options) || !out || !options->session_id || installed.load()) return 0;
    *out = nullptr;
    auto* base = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr));
    unsigned char actual[16]{};
    for (const auto& site : sites) {
        if (!site.expected || !copy(actual, base + site.rva, sizeof(actual)) ||
            std::memcmp(actual, site.expected, sizeof(actual))) {
            char message[128]{};
            std::snprintf(message, sizeof(message), "AC7 native renderer refused: expected entry bytes differ at RVA 0x%x", site.rva);
            if (options->log) options->log(options->user, message);
            return 0;
        }
    }
    const Site helpers[] = {
        {0xfb7a00, 0, "\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x40\x48\x8b\x01\x48\x8b\xda"},
        {0x1126090, 0, "\x83\xb9\xb0\x09\x00\x00\x02\x4c\x8b\x89\x10\x14\x00\x00\x75\x58"},
        {0x12183c0, 0, "\x40\x53\x48\x83\xec\x20\x8b\x1d\x94\x0c\xa6\x02\x85\xdb\x0f\x84"},
        {0x116ea00, 0, "\x4c\x8b\xdc\x55\x56\x57\x41\x56\x48\x81\xec\x78\x0d\x00\x00\x48"},
        {0x109e0d0, 0, "\x4c\x89\x44\x24\x18\x53\x56\x57\x48\x81\xec\xa0\x00\x00\x00\x48"},
        {0xee93f0, 0, "\x48\x89\x5c\x24\x08\x55\x56\x57\x41\x54\x41\x55\x41\x56\x41\x57"},
        {0x923e60, 0, "\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\x0a\xe1\x15\x03\x48\x85"},
        {0x938fb0, 0, "\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\xa2\xb6\x14\x03\x48\x85"},
        {0x925860, 0, "\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\xc2\xc8\x15\x03\x48\x85"},
        {0x925d10, 0, "\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\x22\xc4\x15\x03\x48\x85"},
        {0x925e10, 0, "\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\x3a\xc3\x15\x03\x48\x85"},
        {0x925710, 0, "\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\xea\xc8\x15\x03\x48\x85"},
        {0x8b3100, 0, "\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\x22\x16\x1c\x03\x48\x85"},
        {0x1b495f0, 0, "\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\x52\x6b\x17\x02\x48\x85"},
        {0x8c1bd0, 0, "\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\xb2\x50\x1b\x03\x48\x85"},
        {0xfcf880, 0, "\x48\x8b\xc4\x55\x53\x57\x41\x55\x48\x8d\x68\xa1\x48\x81\xec\x88"},
        {0x1ab0200, 0, "\x0f\xb6\x44\x24\x28\x83\xe0\x01\x89\x91\xd0\x00\x00\x00\x44\x89"},
        {0x1ab7dc0, 0, "\x48\x89\x5c\x24\x10\x57\x48\x83\xec\x40\x48\x83\x79\x70\x00\x0f"},
        {0xde5cf0, 0, "\x40\x53\x48\x83\xec\x20\x48\x8b\xd9\x48\x3b\xca\x0f\x84\xbe\x00"},
        {0x19b6fe0, 0, "\x48\x83\xec\x28\x8b\x41\x08\x45\x32\xc0\x0f\x57\xd2\x4c\x8b\xd9"},
        {0xe93670, 0, "\x48\x89\x5c\x24\x08\x48\x89\x74\x24\x10\x57\x48\x83\xec\x20\x48"},
        {0x10b4870, 0, "\x48\x89\x5c\x24\x10\x48\x89\x6c\x24\x18\x48\x89\x74\x24\x20\x57"},
        {0x109dc70, 0, "\x48\x8d\x05\x99\xcf\xb9\x02\xc3\xcc\xcc\xcc\xcc\xcc\xcc\xcc\xcc"},
        {0x1127a20, 0, "\x48\x89\x5c\x24\x20\x55\x56\x57\x48\x8d\x6c\x24\xb9\x48\x81\xec"},
        {0xff9af0, 0, "\x40\x53\x48\x83\xec\x60\xf3\x0f\x10\x41\x20\x48\x8b\xd9\xf3\x0f"},
        {0x11346d0, 0, "\x48\x8b\xc4\x55\x56\x57\x41\x56\x41\x57\x48\x8d\x68\xb9\x48\x81"},
    };
    // Enlarged separate translucency needs the matching depth and shader-view paths.
    const struct { uint32_t rva; const char* bytes; size_t count; } depth_gates[] = {
        {0x1168f6f, "\x74\x0e", 2}, {0x1097a0c, "\x74\x17", 2},
        {0x109d03a, "\x44\x0f\x45\xf9", 4}, {0x109d061, "\x74\x17", 2},
        {0x11583a6, "\x0f\x2f\x80\x20\x02\x00\x00\x74\x0e", 9},
        {0x1025b12, "\x0f\x2f\x86\x20\x02\x00\x00\x74\x04", 9},
        {0x10294d2, "\x0f\x2f\x86\x20\x02\x00\x00\x74\x04", 9},
        {0x102ada2, "\x0f\x2f\x86\x20\x02\x00\x00\x74\x04", 9},
        {0x102c672, "\x0f\x2f\x86\x20\x02\x00\x00\x74\x04", 9},
        {0x102e792, "\x0f\x2f\x86\x20\x02\x00\x00\x74\x04", 9},
        {0x1031902, "\x0f\x2f\x86\x20\x02\x00\x00\x74\x04", 9}
    };
    for (const auto& gate : depth_gates) {
        if (!copy(actual, base + gate.rva, gate.count) || std::memcmp(actual, gate.bytes, gate.count)) {
            char message[144]{};
            std::snprintf(message, sizeof(message), "AC7 native renderer refused: enlarged UI depth/view gate missing at RVA 0x%x", gate.rva);
            if (options->log) options->log(options->user, message);
            return 0;
        }
    }
    for (const auto& helper : helpers) if (!copy(actual, base + helper.rva, 16) || std::memcmp(actual, helper.expected, 16)) {
        char message[128]{};
        std::snprintf(message, sizeof(message), "AC7 native renderer refused: expected helper bytes differ at RVA 0x%x", helper.rva);
        if (options->log) options->log(options->user, message);
        return 0;
    }
    auto* self = new(std::nothrow) rsf_ac7_native_renderer;
    if (!self) return 0;
    self->options = *options;
    if (!rsf_ac7_render_scopes_create_passes(options->session_id,
        options->pending_capacity ? options->pending_capacity : 256,
        options->on_state, options->on_stage, options->user, &self->scopes)) {
        delete self; return 0;
    }
    const auto init = MH_Initialize();
    if (init != MH_OK && init != MH_ERROR_ALREADY_INITIALIZED) {
        rsf_ac7_render_scopes_quiesce(self->scopes); rsf_ac7_render_scopes_destroy(self->scopes); delete self; return 0;
    }
    bool success = true;
    for (size_t i = 0; i < sizeof(sites) / sizeof(sites[0]); ++i) {
        sites[i].target = base + sites[i].rva;
        if (MH_CreateHook(sites[i].target, hooks[i], &sites[i].original) != MH_OK) { success = false; break; }
        ++self->hooks;
    }
    if (success) {
        installed.store(self, std::memory_order_release);
        for (uint32_t i = 0; i < self->hooks; ++i) if (MH_EnableHook(sites[i].target) != MH_OK) success = false;
    }
    if (!success) {
        self->active.store(false);
        for (uint32_t i = 0; i < self->hooks; ++i) MH_DisableHook(sites[i].target);
        if (entry_calls.load() || outer_calls.load()) {
            // Partial activation can overlap an inactive forwarding call. Pin this failed module
            // instead of unloading code that a native thread will return through.
            HMODULE pinned = nullptr;
            GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                reinterpret_cast<LPCWSTR>(&rsf_ac7_native_renderer_prepare), &pinned);
            self->quiescing.store(true); rsf_ac7_render_scopes_quiesce(self->scopes);
            *out = self; log(*self, "AC7 native preparation failed during a forwarding call; inactive module pinned until exit");
            return 0;
        }
        for (uint32_t i = 0; i < self->hooks; ++i) MH_RemoveHook(sites[i].target);
        installed.store(nullptr);
        rsf_ac7_render_scopes_quiesce(self->scopes); rsf_ac7_render_scopes_destroy(self->scopes); delete self; return 0;
    }
    log(*self, "AC7 native renderer prepared: owned view sizing, pre-tonemap SR graph node, downstream engine descriptors, owned widget raster targets and RHI leases; activation pending");
    *out = self; return 1;
}
catch (...) { return 0; }
extern "C" int rsf_ac7_native_renderer_start(rsf_ac7_native_renderer* self)
{
    if (!self || installed.load() != self || self->quiescing.load() || !apply_scene_precision_patch(*self)) return 0;
    self->active.store(true); return 1;
}
extern "C" int rsf_ac7_native_renderer_is_active(rsf_ac7_native_renderer* self)
{ return self && installed.load(std::memory_order_acquire) == self && self->active.load(std::memory_order_acquire); }
extern "C" void rsf_ac7_native_renderer_quiesce(rsf_ac7_native_renderer* self)
{ if (self) { self->quiescing.store(true); self->active.store(false); rsf_ac7_render_scopes_quiesce(self->scopes); } }
extern "C" int rsf_ac7_native_renderer_stop(rsf_ac7_native_renderer* self)
{
    if (!self) return 1;
    if (self->active.load()) return 0;
    // Keep inactive CPU observers installed until their render-thread-only pool refs retire.
    // Disabling first would remove the only owner that can drain an asynchronous RHI release.
    drain_retired(*self);
    if (entry_calls.load() || self->live_renderers.load() || self->live_window_tasks.load() || self->live_frame_tasks.load() || self->pending_uniforms.load() || self->retired.load() || !rsf_ac7_render_scopes_idle(self->scopes)) return 0;
    if (self->input_hook) {
        if (!UnhookWindowsHookEx(self->input_hook)) return 0;
        self->input_hook = nullptr;
    }
    for (uint32_t i = 0; i < self->hooks; ++i) MH_DisableHook(sites[i].target);
    if (entry_calls.load() || outer_calls.load()) return 0;
    if (self->scene_precision_site) {
        if (!write_render_code(self->scene_precision_site, precise_scene_format, native_scene_format, sizeof(native_scene_format))) {
            log(*self, "native scene-precision patch could not restore; renderer retained"); return 0;
        }
        self->scene_precision_site = nullptr;
    }
    if (self->cloud_resolution_site) {
        if (!write_render_code(self->cloud_resolution_site, full_cloud_divisor, native_cloud_divisor, sizeof(native_cloud_divisor))) {
            log(*self, "native TrueSky cloud-resolution patch could not restore; renderer retained"); return 0;
        }
        self->cloud_resolution_site = nullptr;
    }
    if (self->cloud_motion_call) {
        constexpr unsigned char native_draw[]{0xff,0x90,0x30,0x01,0x00,0x00};
        unsigned char relayed[6]{0xff,0x15};
        const int32_t relative = int32_t(intptr_t(self->cloud_motion_relay) - intptr_t(self->cloud_motion_call + 6));
        std::memcpy(relayed + 2, &relative, 4);
        if (!write_render_code(self->cloud_motion_call, relayed, native_draw, sizeof(native_draw))) {
            log(*self,"native TrueSky cloud motion relay could not restore; renderer retained"); return 0;
        }
        self->cloud_motion_call = nullptr;
        VirtualFree(self->cloud_motion_relay, 0, MEM_RELEASE); self->cloud_motion_relay = nullptr;
        rsf_ac7_cloud_depth_release(self->cloud_motion);
    }
    if (self->cloud_depth_call) {
        constexpr unsigned char native_call[]{0x41,0xff,0x92,0x48,0x01,0x00,0x00};
        if (!write_render_code(self->cloud_depth_call,self->cloud_depth_call_bytes,native_call,sizeof(native_call))) {
            log(*self,"native TrueSky depth dispatch could not restore; renderer retained"); return 0;
        }
        self->cloud_depth_call=nullptr;
    }
    if (self->cloud_depth_format_site) {
        if (!write_render_code(self->cloud_depth_format_site,precise_cloud_depth_format,native_cloud_depth_format,sizeof(native_cloud_depth_format))) {
            log(*self,"native TrueSky depth format could not restore; renderer retained"); return 0;
        }
        self->cloud_depth_format_site=nullptr;
    }
    if (self->cloud_depth_branch) {
        constexpr unsigned char branch[]{0x77}, original[]{0x75};
        if (!write_render_code(self->cloud_depth_branch,branch,original,1)) {
            log(*self,"native TrueSky depth pass selector could not restore; renderer retained"); return 0;
        }
        self->cloud_depth_branch=nullptr;
    }
    if (self->cloud_depth_relay) { VirtualFree(self->cloud_depth_relay,0,MEM_RELEASE); self->cloud_depth_relay=nullptr; }
    if (self->cloud_depth_shader) { self->cloud_depth_shader->Release(); self->cloud_depth_shader=nullptr; }
    if (self->cloud_depth_device) { self->cloud_depth_device->Release(); self->cloud_depth_device=nullptr; }
    self->cloud_depth_ready.store(false);
    if (!rsf_ac7_render_scopes_destroy(self->scopes)) return 0;
    self->scopes = nullptr;
    drain_retired(*self);
    if (self->retired.load()) return 0;
    for (uint32_t i = 0; i < self->hooks; ++i) MH_RemoveHook(sites[i].target);
    installed.store(nullptr); delete self; return 1;
}
extern "C" int rsf_ac7_native_renderer_current(rsf_ac7_native_renderer* self, rsf_ac7_render_scope* out)
{ return self && rsf_ac7_render_scope_current(self->scopes, out); }
