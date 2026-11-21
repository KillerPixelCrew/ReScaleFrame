// SPDX-License-Identifier: MIT
#include "motion_stage.h"
#include "motion_stage_platform.h"
#include "engine_guards.h"
#include "geometry_feed.h"
#include "motion_d3d11.h"
#include "motion_foliage.h"
#include "native_hooks.h"
#include "native_observer.h"
#include "runtime_shaders.h"
#include "shader_d3d.h"
#include "sr_profile.h"
#include "sr_stage.h"
#include <intrin.h>
#include <algorithm>
#include <array>
#include <cstdio>
#include <cstring>
#include <memory>
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

HookSet hooks;
std::array<void*, motion_site_count> originals{};
using Unary = void(__fastcall*)(void*);
using CreateShader = long long(__fastcall*)(const char*, const void*, unsigned, const char*, void**, char);

using DrawIndexedFn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, INT);
using DrawFn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT);
using DrawIndexedInstancedFn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, UINT, INT, UINT);
using DrawInstancedFn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, UINT, UINT);
HookSet draw_hooks;
std::array<void*, 4> draw_originals{};

template<class Call> void around_draw(ID3D11DeviceContext* context, Call&& call) {
    rsf_anomaly_motion_stage_draw(context, [](void* user, void* immediate) {
        (*static_cast<std::remove_reference_t<Call>*>(user))(static_cast<ID3D11DeviceContext*>(immediate));
    }, &call);
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
// ID3D11DeviceContext vtable slots (d3d11.h declaration order): DrawIndexed 12, Draw 13, DrawIndexedInstanced 20,
// DrawInstanced 21.
bool install_draw_hooks(void* context) noexcept {
    if (!context) return false;
    void** vtable{};
    if (!guarded_copy(&vtable, context, sizeof(vtable)) || !vtable) return false;
    const std::array<std::size_t, 4> slots{12, 13, 20, 21};
    const std::array<void*, 4> replacements{reinterpret_cast<void*>(&draw_indexed_hook), reinterpret_cast<void*>(&draw_hook),
        reinterpret_cast<void*>(&draw_indexed_instanced_hook), reinterpret_cast<void*>(&draw_instanced_hook)};
    std::array<ResolvedHookSite, 4> sites{};
    for (std::size_t i = 0; i < slots.size(); ++i) {
        void* target{};
        if (!guarded_copy(&target, vtable + slots[i], sizeof(target))) return false;
        sites[i] = {target, replacements[i], &draw_originals[i]};
    }
    return draw_hooks.install_resolved(sites);
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
    const auto checked = guard_image(static_cast<const std::byte*>(module), image_size);
    return rsf_anomaly_motion_guard_failure(&checked, &guards);
}

rsf_anomaly_shader_bytes bytes(std::string_view s) noexcept {
    return {reinterpret_cast<const uint8_t*>(s.data()), s.size()};
}
rsf_anomaly_shader_bytes bytes(std::span<const std::byte> s) noexcept {
    return {reinterpret_cast<const uint8_t*>(s.data()), s.size()};
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

void shader_created(const char* target, const void* buffer, unsigned size, const char* file,
    void** result, const void* return_address, const void* caller_rsp) noexcept {
    char stage{};
    if (!guarded_copy(&stage, target, 1) || (stage != 'v' && stage != 'p' && stage != 'h' && stage != 'd') ||
        !buffer || !size) return;
    void* engine_object{};
    void* stock{};
    if (!guarded_copy(&engine_object, result, sizeof(engine_object)) || !engine_object ||
        !guarded_copy(&stock, static_cast<std::byte*>(engine_object) + shader_object_offset, sizeof(stock)) || !stock) return;
    char filename[520];
    std::size_t len = 0;
    if (file) for (; len < sizeof(filename); ++len)
        if (!guarded_copy(filename + len, file + len, 1) || !filename[len]) break;
    rsf_anomaly_motion_stage_shader(static_cast<uint8_t>(stage), target, buffer, size, bytes(std::string_view(filename, len)),
        stock, return_address, caller_rsp);
}
long long __fastcall create_shader_hook(const char* target, const void* buffer, unsigned size, const char* file,
    void** result, char disasm) {
    rsf_anomaly_motion_stage_attach();
    const auto hr = reinterpret_cast<CreateShader>(originals[site_create_shader])(target, buffer, size, file, result, disasm);
    if (static_cast<int32_t>(hr) >= 0 && rsf_anomaly_motion_stage_admits())
        shader_created(target, buffer, size, file, result, _ReturnAddress(),
            static_cast<const std::byte*>(_AddressOfReturnAddress()) + 8);
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
    if (hooks.created()) return hooks.enable();
    const auto& s = mp().motion_sites;
    const std::array<HookSite, motion_site_count> sites{{
        {s[site_create_shader].name, s[site_create_shader].rva, s[site_create_shader].expected,
            reinterpret_cast<void*>(&create_shader_hook), &originals[site_create_shader]},
        {s[site_phase_scene_begin].name, s[site_phase_scene_begin].rva, s[site_phase_scene_begin].expected,
            reinterpret_cast<void*>(&scene_begin_hook), &originals[site_phase_scene_begin]},
        {s[site_phase_scene_end].name, s[site_phase_scene_end].rva, s[site_phase_scene_end].expected,
            reinterpret_cast<void*>(&scene_end_hook), &originals[site_phase_scene_end]}}};
    return hooks.install(image, size, sites);
}
void* render_target() noexcept {
    std::byte* target{};
    return read_global(srp().render_implementation_rva + sr::render_target_offset, target) ? target : nullptr;
}
bool position_views(void* target, PositionViews* views) noexcept {
    constexpr auto position_offset = sr::target_slots[0].offset;
    static_assert(position_offset == 0x378);
    std::byte* crt{};
    return guarded_copy(&crt, static_cast<std::byte*>(target) + position_offset, sizeof(crt)) && crt &&
        guarded_copy(&views->rtv, crt + sr::crt_rtv_offset, sizeof(views->rtv)) &&
        guarded_copy(&views->surface, crt + sr::crt_surface_offset, sizeof(views->surface));
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
bool target_facts(void* surface, TargetFacts* out) noexcept {
    ComPtr<ID3D11Texture2D> texture;
    if (FAILED(static_cast<IUnknown*>(surface)->QueryInterface(IID_PPV_ARGS(&texture)))) return false;
    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    *out = {desc.Width, desc.Height, desc.SampleDesc.Count, desc.ArraySize};
    return true;
}
bool camera(float (*view)[16], float (*projection)[16]) noexcept {
    const auto* xforms = image + srp().backend_rva + sr::backend_xforms_offset;
    return guarded_copy(view, xforms + sr::xforms_view_offset, sizeof(*view)) &&
        guarded_copy(projection, xforms + sr::xforms_project_offset, sizeof(*projection));
}

template<class Shader, auto Get>
void* bound_shader(ID3D11DeviceContext* context) noexcept {
    ComPtr<Shader> shader;
    (context->*Get)(&shader, nullptr, nullptr);
    // The caller is inside constants.flush. The engine retains its stock shader.
    return shader.Get();
}
void bound_shaders(void* raw, ShaderObjects* out, bool all_stages) noexcept {
    auto* c = static_cast<ID3D11DeviceContext*>(raw);
    const auto vs = bound_shader<ID3D11VertexShader, &ID3D11DeviceContext::VSGetShader>(c);
    if (!all_stages) { *out = {vs, nullptr, nullptr, nullptr, nullptr}; return; }
    *out = {
        vs,
        bound_shader<ID3D11HullShader, &ID3D11DeviceContext::HSGetShader>(c),
        bound_shader<ID3D11DomainShader, &ID3D11DeviceContext::DSGetShader>(c),
        bound_shader<ID3D11PixelShader, &ID3D11DeviceContext::PSGetShader>(c),
        bound_shader<ID3D11GeometryShader, &ID3D11DeviceContext::GSGetShader>(c)
    };
}
void set_shaders(void* raw, const ShaderObjects* shader, bool tess) noexcept {
    auto* c = static_cast<ID3D11DeviceContext*>(raw);
    c->VSSetShader(static_cast<ID3D11VertexShader*>(shader->vs), nullptr, 0);
    c->PSSetShader(static_cast<ID3D11PixelShader*>(shader->ps), nullptr, 0);
    if (tess) {
        c->HSSetShader(static_cast<ID3D11HullShader*>(shader->hs), nullptr, 0);
        c->DSSetShader(static_cast<ID3D11DomainShader*>(shader->ds), nullptr, 0);
    }
}
constexpr rsf_anomaly_motion_bind_platform bind_platform{
    [](void*, const void* address, void* out, size_t size) -> uint32_t { return guarded_copy(out, address, size); },
    [](void*, void* address, const void* in, size_t size) -> uint32_t { return guarded_copy(address, in, size); },
    [](void* context, void* const* targets, void* depth) {
        if (context) static_cast<ID3D11DeviceContext*>(context)->OMSetRenderTargets(4,
            reinterpret_cast<ID3D11RenderTargetView* const*>(targets), static_cast<ID3D11DepthStencilView*>(depth));
    },
    [](void* context) { if (context) static_cast<ID3D11DeviceContext*>(context)->OMSetRenderTargets(0, nullptr, nullptr); }
};

void* open_pack(void* module, const void* profile, const wchar_t* pack, const wchar_t* cache,
    PackInfo* info, char* message, size_t size) noexcept try {
    std::string error;
    if (auto* result = open_runtime_pack(pack, cache, static_cast<std::byte*>(module),
            *static_cast<const BuildProfile*>(profile)->motion, error)) {
        *info = {rsf_anomaly_runtime_pack_manifest(result)};
        return result;
    }
    std::snprintf(message, size, "%s", error.c_str());
    return nullptr;
} catch (...) { return nullptr; }
void prepare_shader(void* pack, const rsf_anomaly_shader_request* request,
    const rsf_anomaly_shader_entry* root, ShaderResult* out) noexcept try {
    D3DShaderReflector reflector(rsf_anomaly_runtime_pack_reflector(pack));
    auto storage = std::make_unique<Prepared>(prepare_runtime_pack(pack, *request, manifest_entry(*root), reflector));
    *out = {storage.get(), bytes(storage->detail), storage->skin, static_cast<uint8_t>(storage->outcome)};
    storage.release();
} catch (...) {}
void* create_shader(void* pack, const char* target, void* device, void* reflection, bool attach, bool* refused) noexcept {
    const auto& api = runtime_pack_api(pack);
    if (attach && api.attach_d3d11(device) != RSF_PATCH_OK) { *refused = true; return nullptr; }
    void* shader{};
    auto& buffer = *static_cast<Prepared*>(reflection);
    const auto result = api.create_d3d11(device, target, {buffer.variant.data(), buffer.variant.size()}, &shader);
    if (result == RSF_PATCH_OK) {
        // The installed draw needs only the replacement interface.
        std::vector<std::byte>().swap(buffer.variant);
        buffer.stock = {};
        buffer.entry = nullptr;
        return shader;
    }
    if (shader) static_cast<IUnknown*>(shader)->Release();
    return nullptr;
}
const MotionPlatform platform{
    sizeof(MotionPlatform), motion_platform_abi, &motion_device_platform(), &bind_platform,
    {sr::backend_rt_offset, sr::backend_zb_offset, sr::backend_rt_changed_offset},
    guard_failure,
    geometry::geometry_hooks_active,
    [](bool enabled) { geometry::geometry_set_flush_callback(enabled ? flush : nullptr); },
    geometry::geometry_enable_foliage_identity, install_hooks, [] { return hooks.disable(); }, [] { return hooks.created(); },
    install_draw_hooks, render_target, position_views, native_frame, render_view, reset_generation,
    options_word3, target_facts, backend, hw_context, hw_device,
    [](void* c, void* rtv) {
        static constexpr float zero[4]{};
        static_cast<ID3D11DeviceContext*>(c)->ClearRenderTargetView(static_cast<ID3D11RenderTargetView*>(rtv), zero);
    },
    bound_shaders, set_shaders,
    &motion_replay_platform(),
    camera, [](float (*world)[16]) { return geometry::read_world_matrix(*world); },
    geometry::read_vertex_constants, geometry::write_vertex_constants,
    [](void* raw, MotionDrawState* out) {
        D3D11_VIEWPORT viewport{};
        UINT count = 1;
        static_cast<ID3D11DeviceContext*>(raw)->RSGetViewports(&count, &viewport);
        const auto jitter = sr::current_jitter_uv();
        *out = {{viewport.TopLeftX, viewport.TopLeftY, viewport.Width, viewport.Height},
            {jitter.current_u, jitter.current_v, jitter.previous_u, jitter.previous_v}};
    },
    open_pack, rsf_anomaly_runtime_pack_close,
    [](void* pack, void* device) { runtime_pack_api(pack).attach_d3d11(device); },
    [](void* pack, const wchar_t* path) { return runtime_pack_api(pack).dump(path); },
    [](void* pack, PatchCounters* out) {
        rsf_patch_counters raw{sizeof(raw), RSF_SHADER_PATCH_VERSION};
        runtime_pack_api(pack).counters(&raw);
        *out = {raw.intercepted, raw.built, raw.cache_hits, raw.refused, {}};
        std::copy(std::begin(raw.refusals), std::end(raw.refusals), out->refusals);
    },
    [](rsf_anomaly_shader_frame_layout* out) -> void* { *out = shader_frame_layout(mp()); return image; },
    [](const void*, uintptr_t address, uint8_t* out, size_t size) -> bool {
        return guarded_copy(out, reinterpret_cast<const void*>(address), size);
    },
    prepare_shader, [](void* reflection) { delete static_cast<Prepared*>(reflection); },
    [](void* raw, uint8_t family, FoliageLayout* out) {
        *out = make_foliage_layout(static_cast<Family>(family), static_cast<Prepared*>(raw)->stock);
        return out->usable;
    },
    [](const void* vs, const void* hs, const void* ds, const void* ps) {
        const auto reflection = [](const void* raw) -> const Reflection& {
            return static_cast<const Prepared*>(raw)->reflection;
        };
        return hs ? tess_chain_linkable(reflection(vs), reflection(hs), reflection(ds), reflection(ps)) :
            linkable(reflection(vs), reflection(ps));
    },
    create_shader
};

static_assert(sizeof(PositionViews) == 16 && sizeof(TargetFacts) == 16 && sizeof(DrawFacts) == 32);
static_assert(sizeof(ShaderObjects) == 40 && sizeof(ShaderResult) == 32 && sizeof(PackInfo) == 8);
static_assert(sizeof(FoliageLayout) == 108 && offsetof(FoliageLayout, xform) == 4 && offsetof(FoliageLayout, usable) == 104);
static_assert(sizeof(MotionPlatform) == 352 && offsetof(MotionPlatform, guards) == 40 &&
    offsetof(MotionPlatform, render_target) == 104 && offsetof(MotionPlatform, target_facts) == 152 &&
    offsetof(MotionPlatform, frame_layout) == 296 && offsetof(MotionPlatform, create_shader) == 344);
static_assert(sizeof(FrameTarget) == 24 && offsetof(FrameTarget, main) == 20);
static_assert(static_cast<uint8_t>(ReplayResult::Done) == 0 && static_cast<uint8_t>(ReplayResult::SkippedNoDepthWrite) == 1);
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
