// SPDX-License-Identifier: MIT
#include "motion_stage.h"
#include "motion_stage_platform.h"
#include "engine_guards.h"
#include "geometry_feed.h"
#include "native_hooks.h"
#include "native_memory.h"
#include "native_observer.h"
#include "stage_entries.h"
#include "sr_profile.h"
#include "sr_stage.h"
#include <d3dcompiler.h>
#include <intrin.h>
#include <array>
#include <type_traits>

namespace rsf::anomaly::motion {
// Borrowed motion guard facts for the selected build. Each array pointer borrows the full profile array; Rust
// (engine_guards/motion.rs) owns the check order and the refusal names.
struct MotionGuardProfile {
    const NativeSite* sites;
    const UniqueGuard* unique;
    size_t unique_count;
    const NativeSite* create_shader_calls;
    const TessCreateSite* tess_create_sites;
    const char* shader_compile_prologue;
    const char* d3dcompile_setup;
    const char* tess_dispatch;
    uint32_t shader_compile_rva;
    uint32_t d3dcompile_setup_rva;
    uint32_t tess_dispatch_rva;
    uint32_t source_name_rva;
};
static_assert(sizeof(NativeSite) == 32 && offsetof(NativeSite, rva) == 8 && offsetof(NativeSite, expected) == 12);
static_assert(sizeof(UniqueGuard) == 24 && offsetof(UniqueGuard, rva) == 8 && offsetof(UniqueGuard, expected_hex) == 16);
static_assert(sizeof(TessCreateSite) == 40 && offsetof(TessCreateSite, prologue) == 16 &&
    offsetof(TessCreateSite, wrapper_rva) == 24 && offsetof(TessCreateSite, wrapper) == 32);
static_assert(sizeof(MotionGuardProfile) == 80 && offsetof(MotionGuardProfile, create_shader_calls) == 24 &&
    offsetof(MotionGuardProfile, shader_compile_prologue) == 40 && offsetof(MotionGuardProfile, shader_compile_rva) == 64 &&
    offsetof(MotionGuardProfile, source_name_rva) == 76);
extern "C" const char* rsf_anomaly_motion_guard_failure(const rsf_anomaly_guard_image*, const MotionGuardProfile*) noexcept;

namespace {
std::byte* image{};
// The exact build install() was given (build_profile.h), set together with `image`. Until then every read fails on the null
// image and the empty tables are never dereferenced into engine memory.
const BuildProfile* build{};
const MotionProfile& mp() noexcept { static constexpr MotionProfile none{}; return build ? *build->motion : none; }
const sr::SrProfile& srp() noexcept { static constexpr sr::SrProfile none{}; return build ? *build->sr : none; }
const NativeProfile& np() noexcept { static constexpr NativeProfile none{}; return build ? *build->native : none; }
template<class T> bool read_global(uint32_t rva, T& value) noexcept { return image && guarded_copy(&value, image + rva, sizeof(T)); }

rsf_anomaly_hook_set hooks{};
std::array<void*, motion_site_count> originals{};
using Unary = void(__fastcall*)(void*);
using CreateShader = long long(__fastcall*)(const char*, const void*, unsigned, const char*, void**, char);

using DrawIndexedFn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, INT);
using DrawFn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT);
using DrawIndexedInstancedFn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, UINT, INT, UINT);
using DrawInstancedFn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, UINT, UINT);
rsf_anomaly_hook_set draw_hooks{};
std::array<void*, 4> draw_originals{};

static_assert(sizeof(MotionDrawScope) == 176 && alignof(MotionDrawScope) == 8 &&
    sizeof(rsf_anomaly_motion_replay_scope) == 2184 && alignof(rsf_anomaly_motion_replay_scope) == 8);
static_assert(NoMotionDraw == 0 && RestoreMotionDraw == 1 && ReplayMotionDraw == 2);

template<class Call> void replay_draw(const MotionDrawScope& draw, ID3D11DeviceContext* context, Call& call) {
    rsf_anomaly_motion_replay_scope replay;
    if (!rsf_anomaly_motion_stage_replay_begin(&draw, &replay)) return;
    struct Restore {
        const MotionDrawScope* draw;
        rsf_anomaly_motion_replay_scope* replay;
        bool completed{};
        ~Restore() { rsf_anomaly_motion_stage_replay_end(draw, replay, completed); }
    } restore{&draw, &replay};
    call(context);
    restore.completed = true;
}

template<class Call> void around_draw(ID3D11DeviceContext* context, Call&& call) {
    MotionDrawScope scope;
    const auto route = rsf_anomaly_motion_stage_draw_begin(context, &scope);
    struct Restore {
        MotionDrawScope* scope;
        bool initialized;
        bool completed{};
        ~Restore() { if (initialized) rsf_anomaly_motion_stage_draw_end(scope, completed); }
    } restore{&scope, route != NoMotionDraw};
    call(context);
    restore.completed = true;
    if (route == ReplayMotionDraw) replay_draw(scope, context, call);
}
void STDMETHODCALLTYPE draw_indexed_hook(ID3D11DeviceContext* c, UINT count, UINT start, INT base) {
    around_draw(c, [&](ID3D11DeviceContext* x) { reinterpret_cast<DrawIndexedFn>(draw_originals[0])(x, count, start, base); });
}
void STDMETHODCALLTYPE draw_hook(ID3D11DeviceContext* c, UINT count, UINT start) {
    around_draw(c, [&](ID3D11DeviceContext* x) { reinterpret_cast<DrawFn>(draw_originals[1])(x, count, start); });
}
void STDMETHODCALLTYPE draw_indexed_instanced_hook(ID3D11DeviceContext* c, UINT count, UINT instances, UINT start, INT base, UINT first) {
    around_draw(c, [&](ID3D11DeviceContext* x) {
        reinterpret_cast<DrawIndexedInstancedFn>(draw_originals[2])(x, count, instances, start, base, first);
    });
}
void STDMETHODCALLTYPE draw_instanced_hook(ID3D11DeviceContext* c, UINT count, UINT instances, UINT start, UINT first) {
    around_draw(c, [&](ID3D11DeviceContext* x) {
        reinterpret_cast<DrawInstancedFn>(draw_originals[3])(x, count, instances, start, first);
    });
}
bool install_draw_hooks(void* context) noexcept {
    void* targets[4]{};
    if (!rsf_anomaly_motion_resolve_draws(guarded_copy, context, &targets)) return false;
    const std::array<void*, 4> replacements{reinterpret_cast<void*>(&draw_indexed_hook), reinterpret_cast<void*>(&draw_hook),
        reinterpret_cast<void*>(&draw_indexed_instanced_hook), reinterpret_cast<void*>(&draw_instanced_hook)};
    std::array<rsf_anomaly_hook_target, 4> sites{};
    for (std::size_t i = 0; i < sites.size(); ++i) {
        sites[i] = {targets[i], replacements[i], &draw_originals[i]};
    }
    return rsf_anomaly_hooks_install_resolved(&draw_hooks, sites.data(), sites.size());
}

// Lends the selected build's tables; Rust returns the first mismatching check's name or null.
const char* guard_failure(void* module, uint32_t image_size, const void* selected) noexcept {
    const auto& profile = *static_cast<const BuildProfile*>(selected)->motion;
    static_assert(motion_site_count == 3 && std::tuple_size_v<decltype(MotionProfile::create_shader_calls)> == 2 &&
        std::tuple_size_v<decltype(MotionProfile::tess_create_sites)> == 2);
    const MotionGuardProfile guards{
        .sites = profile.motion_sites.data(),
        .unique = profile.motion_unique_guards.data(),
        .unique_count = profile.motion_unique_guards.size(),
        .create_shader_calls = profile.create_shader_calls.data(),
        .tess_create_sites = profile.tess_create_sites.data(),
        .shader_compile_prologue = profile.shader_compile_prologue,
        .d3dcompile_setup = profile.d3dcompile_setup,
        .tess_dispatch = profile.tess_dispatch,
        .shader_compile_rva = profile.shader_compile_rva,
        .d3dcompile_setup_rva = profile.d3dcompile_setup_rva,
        .tess_dispatch_rva = profile.tess_dispatch_rva,
        .source_name_rva = profile.source_name_rva,
    };
    const rsf_anomaly_guard_image checked{static_cast<const uint8_t*>(module), image_size, guarded_copy};
    return rsf_anomaly_motion_guard_failure(&checked, &guards);
}

void* hw_device() noexcept {
    void* device{};
    read_global(srp().hw_device_rva, device);
    return device;
}
void* hw_context() noexcept {
    void* context{};
    read_global(srp().hw_context_rva, context);
    return context;
}
void* backend() noexcept { return image ? image + srp().backend_rva : nullptr; }

static_assert(shader_object_offset == 0x10 && sizeof(ShaderCreation) == 56 &&
    offsetof(ShaderCreation, return_address) == 32 && offsetof(ShaderCreation, status) == 48);
long long __fastcall create_shader_hook(const char* target, const void* buffer, unsigned size, const char* file,
    void** result, char disasm) {
    rsf_anomaly_motion_stage_attach();
    const auto hr = reinterpret_cast<CreateShader>(originals[site_create_shader])(target, buffer, size, file, result, disasm);
    const ShaderCreation creation{target, buffer, file, result, _ReturnAddress(),
        static_cast<const std::byte*>(_AddressOfReturnAddress()) + 8, static_cast<int32_t>(hr), size};
    rsf_anomaly_motion_stage_shader(&creation);
    return hr;
}
void __fastcall scene_begin_hook(void* self) {
    rsf_anomaly_motion_stage_cancel_draw();
    reinterpret_cast<Unary>(originals[site_phase_scene_begin])(self);
    rsf_anomaly_motion_stage_open(self);
}
void __fastcall scene_end_hook(void* self) {
    rsf_anomaly_motion_stage_close();
    reinterpret_cast<Unary>(originals[site_phase_scene_end])(self);
}
void flush(const geometry::DrawTimeFacts& facts) noexcept {
    const auto capture = facts.capture ? geometry::snapshot_facts(*facts.capture) : geometry::SnapshotFacts{};
    const DrawFacts draw{facts.capture ? &capture : nullptr, facts.visual, facts.tree_lod,
        static_cast<uint8_t>(facts.pass), static_cast<uint8_t>(facts.kind), static_cast<uint8_t>(facts.classification.verdict),
        facts.view ? static_cast<uint8_t>(*facts.view) : uint8_t{255},
        facts.domain ? static_cast<uint8_t>(*facts.domain) : uint8_t{255}, facts.in_visual, facts.detail};
    rsf_anomaly_motion_stage_flush(&draw);
}
bool install_hooks(void* module, uint32_t size, const void* profile) noexcept {
    build = static_cast<const BuildProfile*>(profile);
    image = static_cast<std::byte*>(module);
    if (rsf_anomaly_hooks_created(&hooks)) return rsf_anomaly_hooks_enable(&hooks);
    const auto& s = mp().motion_sites;
    const auto site = [&s](std::size_t index, void* detour) {
        rsf_anomaly_hook_site hook{s[index].name, s[index].rva, {}, detour, &originals[index]};
        std::memcpy(hook.expected, s[index].expected.data(), sizeof(hook.expected));
        return hook;
    };
    const std::array<rsf_anomaly_hook_site, motion_site_count> sites{{
        site(site_create_shader, reinterpret_cast<void*>(&create_shader_hook)),
        site(site_phase_scene_begin, reinterpret_cast<void*>(&scene_begin_hook)),
        site(site_phase_scene_end, reinterpret_cast<void*>(&scene_end_hook))}};
    return rsf_anomaly_hooks_install(&hooks, image, size, sites.data(), sites.size());
}
void* render_target() noexcept {
    std::byte* target{};
    return read_global(srp().render_implementation_rva + sr::render_target_offset, target) ? target : nullptr;
}
bool position_views(void* target, PositionViews* views) noexcept {
    constexpr auto position_offset = sr::target_slots[0].offset;
    static_assert(position_offset == 0x378);
    return rsf_anomaly_motion_position_views(guarded_copy, target, position_offset,
        sr::crt_rtv_offset, sr::crt_surface_offset, views);
}
bool native_frame(uint32_t* frame) noexcept { return read_global(np().device_frame_rva, *frame); }
bool render_view(uint8_t* out) noexcept {
    const auto view = geometry::current_render_view(image, *build);
    if (!view) return false;
    *out = static_cast<uint8_t>(*view);
    return true;
}
uint64_t reset_generation() noexcept {
    NativeObservation observation{sizeof(observation), observation_abi};
    return rsf_anomaly_observation(&observation) == RSF_OK ? observation.resets : 0;
}
bool options_word3(uint32_t* options) noexcept { return read_global(srp().render_options_word3_rva, *options); }
bool camera(float (*view)[16], float (*projection)[16]) noexcept {
    const auto* xforms = image + srp().backend_rva + sr::backend_xforms_offset;
    return guarded_copy(view, xforms + sr::xforms_view_offset, sizeof(*view)) &&
        guarded_copy(projection, xforms + sr::xforms_project_offset, sizeof(*projection));
}

constexpr rsf_anomaly_motion_bind_platform bind_platform{
    [](void*, const void* address, void* out, size_t size) -> uint32_t { return guarded_copy(out, address, size); },
    [](void*, void* address, const void* in, size_t size) -> uint32_t { return guarded_copy(address, in, size); },
    rsf_anomaly_motion_bind_targets, rsf_anomaly_motion_clear_targets
};

bool include_open(void* table, const char* name, const void** data, uint32_t* size) noexcept {
    __try {
        auto* object = reinterpret_cast<ID3DInclude*>(&table);
        return SUCCEEDED(object->Open(D3D_INCLUDE_LOCAL, name, nullptr, data, size));
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

void include_close(void* table, const void* data) noexcept {
    __try { reinterpret_cast<ID3DInclude*>(&table)->Close(data); }
    __except (EXCEPTION_EXECUTE_HANDLER) {}
}

constexpr rsf_anomaly_engine_include_ops include_ops{include_open, guarded_copy, include_close};

const MotionPlatform platform{
    sizeof(MotionPlatform), motion_platform_abi, rsf_anomaly_motion_device_platform_get(), &bind_platform,
    {sr::backend_rt_offset, sr::backend_zb_offset, sr::backend_rt_changed_offset},
    guard_failure,
    geometry::geometry_hooks_active,
    [](bool enabled) { geometry::geometry_set_flush_callback(enabled ? flush : nullptr); },
    geometry::geometry_enable_foliage_identity, install_hooks, [] { return rsf_anomaly_hooks_disable(&hooks); }, [] { return rsf_anomaly_hooks_created(&hooks); },
    install_draw_hooks, render_target, position_views, native_frame, render_view, reset_generation,
    options_word3, rsf_anomaly_motion_target_facts, backend, hw_context, hw_device,
    rsf_anomaly_motion_clear_target,
    rsf_anomaly_motion_bound_shaders, rsf_anomaly_motion_set_shaders,
    rsf_anomaly_motion_replay_platform_get(),
    camera, [](float (*world)[16]) { return geometry::read_world_matrix(*world); },
    geometry::read_vertex_constants, geometry::write_vertex_constants,
    [](void* raw, MotionDrawState* out) {
        rsf_anomaly_motion_viewport(raw, &out->viewport);
        const auto jitter = sr::current_jitter_uv();
        out->jitter[0] = jitter.current_u;
        out->jitter[1] = jitter.current_v;
        out->jitter[2] = jitter.previous_u;
        out->jitter[3] = jitter.previous_v;
    },
    &include_ops,
    [](const void* profile, rsf_anomaly_engine_shader_profile* out) {
        static_assert(sizeof(wchar_t) == sizeof(uint16_t));
        const auto& motion = *static_cast<const BuildProfile*>(profile)->motion;
        *out = {reinterpret_cast<const uint16_t*>(engine_compiler_module), motion.d3dcompile_setup_rva};
    },
    [](rsf_anomaly_shader_frame_layout* out) -> void* { *out = shader_frame_layout(mp()); return image; },
    [](const void*, uintptr_t address, uint8_t* out, size_t size) -> bool {
        return guarded_copy(out, reinterpret_cast<const void*>(address), size);
    },
};

static_assert(sizeof(PositionViews) == 16 && sizeof(TargetFacts) == 16 && sizeof(DrawFacts) == 32);
static_assert(sizeof(ShaderObjects) == 40);
static_assert(sizeof(rsf_anomaly_engine_include_ops) == 24 &&
    offsetof(rsf_anomaly_engine_include_ops, close) == 16);
static_assert(sizeof(rsf_anomaly_engine_shader_profile) == 16 &&
    offsetof(rsf_anomaly_engine_shader_profile, setup_rva) == 8);
static_assert(sizeof(MotionPlatform) == 288 && offsetof(MotionPlatform, guards) == 40 &&
    offsetof(MotionPlatform, render_target) == 104 && offsetof(MotionPlatform, target_facts) == 152 &&
    offsetof(MotionPlatform, includes) == 256 && offsetof(MotionPlatform, shader_profile) == 264 &&
    offsetof(MotionPlatform, frame_layout) == 272 && offsetof(MotionPlatform, read_engine) == 280);
static_assert(sizeof(FrameTarget) == 24 && offsetof(FrameTarget, written) == 20 && offsetof(FrameTarget, main) == 21);
static_assert(RSF_PHASE_RENDER_SUBMIT == 2 && RSF_DUMP_VIEW_DENSE_MOTION == 7);
}

void configure(const wchar_t* ini, rsf_game_log_fn log, void* user) noexcept {
    rsf_anomaly_motion_stage_configure(&platform, ini, log, user);
}
bool install(std::byte* module, uint32_t size, const BuildProfile& selected) noexcept {
    return rsf_anomaly_motion_stage_install(&platform, module, size, &selected, selected.image_size(), selected.sha256);
}
bool quiesce() noexcept { return rsf_anomaly_motion_stage_quiesce(); }
bool active() noexcept { return rsf_anomaly_motion_stage_enabled(); }
void report(void (*emit)(void*, const char*), void* user) noexcept { rsf_anomaly_motion_stage_report(emit, user); }
FrameTarget frame_target() noexcept {
    FrameTarget target;
    rsf_anomaly_motion_stage_frame_target(&target);
    return target;
}
}

// C entries for the bootstrap and observer leaves (stage_entries.h).
extern "C" void rsf_anomaly_motion_configure_native(const wchar_t* ini, rsf_game_log_fn log) noexcept {
    rsf::anomaly::motion::configure(ini, log, nullptr);
}
extern "C" bool rsf_anomaly_motion_install_native(void* image, size_t build) noexcept {
    if (build >= rsf::anomaly::build_profiles.size()) return false;
    const auto& selected = rsf::anomaly::build_profiles[build];
    return rsf::anomaly::motion::install(static_cast<std::byte*>(image), selected.image_size(), selected);
}
