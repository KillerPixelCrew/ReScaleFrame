// SPDX-License-Identifier: MIT
#include "motion_stage.h"
#include "motion_stage_platform.h"
#include "engine_guards.h"
#include "geometry_feed.h"
#include "geometry_retirement.h"
#include "motion_d3d11.h"
#include "motion_foliage.h"
#include "native_hooks.h"
#include "native_observer.h"
#include "runtime_shaders.h"
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
geometry::RetirementQueue retirements;
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

bool guards_match(std::byte* module, uint32_t image_size, const MotionProfile& profile, const char** failed) noexcept {
    const auto checked = guard_image(module, image_size);
    for (const auto& site : profile.motion_sites) {
        if (!matches(checked, site) || !unique_guards_match(checked, profile.motion_unique_guards, site.rva)) {
            *failed = site.name; return false;
        }
    }
    for (const auto& site : profile.create_shader_calls) {
        if (!matches(checked, site)) { *failed = site.name; return false; }
    }
    if (!rsf_anomaly_guard_hex(&checked, profile.shader_compile_rva, profile.shader_compile_prologue)) { *failed = "CRender::shader_compile prologue"; return false; }
    if (!rsf_anomaly_guard_hex(&checked, profile.d3dcompile_setup_rva, profile.d3dcompile_setup)) { *failed = "CRender::shader_compile D3DCompile setup"; return false; }
    if (!rsf_anomaly_guard_hex(&checked, profile.tess_dispatch_rva, profile.tess_dispatch)) { *failed = "create_shader hull/domain dispatch"; return false; }
    for (const auto& site : profile.tess_create_sites) {
        if (!rsf_anomaly_guard_hex(&checked, site.rva, site.prologue)) { *failed = site.name; return false; }
        if (!rsf_anomaly_guard_hex(&checked, site.wrapper_rva, site.wrapper)) { *failed = "CreateHWShader (hull/domain)"; return false; }
    }
    if (!rsf_anomaly_guard_hex(&checked, profile.source_name_rva, "00")) { *failed = "D3DCompile pSourceName"; return false; }
    return true;
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
bool window_facts(void* target, WindowFacts* facts) noexcept {
    auto* device = hw_device();
    auto* context = hw_context();
    std::byte* engine_target{};
    if (!target || !device || !context || !read_global(srp().render_implementation_rva + sr::render_target_offset, engine_target) ||
        engine_target != target) return false;
    constexpr auto position_offset = sr::target_slots[0].offset;
    static_assert(position_offset == 0x378);
    std::byte* crt{};
    void* rtv{};
    void* surface{};
    if (!guarded_copy(&crt, engine_target + position_offset, sizeof(crt)) || !crt ||
        !guarded_copy(&rtv, crt + sr::crt_rtv_offset, sizeof(rtv)) ||
        !guarded_copy(&surface, crt + sr::crt_surface_offset, sizeof(surface)) || !rtv || !surface) return false;
    uint32_t frame{};
    if (!read_global(np().device_frame_rva, frame)) return false;
    const auto view = geometry::current_render_view(image, *build);
    if (!view) return false;
    NativeObservation observation{sizeof(observation), observation_abi};
    const auto generation = rsf_anomaly_observation(&observation) == RSF_OK ? observation.resets : 0;
    *facts = {device, context, backend(), rtv, surface, generation, frame, static_cast<uint8_t>(*view)};
    return true;
}
int32_t target_facts(void* surface, TargetFacts* out) noexcept {
    uint32_t options3{};
    if (!read_global(srp().render_options_word3_rva, options3) || (options3 & sr::dx10_msaa_bit))
        return RSF_MOTION_MULTISAMPLED;
    ComPtr<ID3D11Texture2D> texture;
    if (FAILED(static_cast<IUnknown*>(surface)->QueryInterface(IID_PPV_ARGS(&texture)))) return RSF_MOTION_FAILED;
    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    *out = {desc.Width, desc.Height, desc.SampleDesc.Count, desc.ArraySize};
    return RSF_MOTION_READY;
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

struct RequestStorage {
    ShaderRequest request;
    std::vector<std::byte> copied;
};
D3DShaderApi& reflector() {
    static D3DShaderApi api(engine_compiler_module);
    return api;
}
void* open_pack(void* module, const void* profile, const wchar_t* pack, const wchar_t* cache,
    PackInfo* info, char* message, size_t size) noexcept try {
    auto result = std::make_unique<RuntimeShaders>();
    std::string error;
    if (!reflector().ready()) error = "engine D3DCompiler unavailable";
    else if (result->open(pack, cache, static_cast<std::byte*>(module), *static_cast<const BuildProfile*>(profile)->motion, error)) {
        *info = {result->manifest()};
        return result.release();
    }
    std::snprintf(message, size, "%s", error.c_str());
    return nullptr;
} catch (...) { return nullptr; }
void* decode_request(const char* target, const void* buffer, uint32_t size, const void* ret, const void* stack,
    uint8_t* status, rsf_anomaly_shader_bytes* source) noexcept try {
    auto storage = std::make_unique<RequestStorage>();
    auto& request = storage->request;
    for (std::size_t i = 0; i < 16; ++i) {
        char c{};
        if (!guarded_copy(&c, target + i, 1) || !c) break;
        request.target.push_back(c);
    }
    request.bytecode = {static_cast<const std::byte*>(buffer), size};
    const auto reader = [](const void* address, void* out, std::size_t count) { return guarded_copy(out, address, count); };
    *status = static_cast<uint8_t>(read_request(mp(), image, ret, static_cast<const std::byte*>(stack), reader, request));
    *source = bytes(request.source);
    return storage.release();
} catch (...) { return nullptr; }
bool copy_bytecode(void* raw) noexcept try {
    auto& storage = *static_cast<RequestStorage*>(raw);
    storage.copied.resize(storage.request.bytecode.size());
    if (!guarded_copy(storage.copied.data(), storage.request.bytecode.data(), storage.copied.size())) return false;
    storage.request.bytecode = storage.copied;
    return true;
} catch (...) { return false; }
void prepare_shader(void* pack, void* raw, const rsf_anomaly_shader_entry* root, ShaderResult* out) noexcept try {
    const auto& request = static_cast<RequestStorage*>(raw)->request;
    auto storage = std::make_unique<Prepared>(
        static_cast<RuntimeShaders*>(pack)->prepare(request, manifest_entry(*root), reflector()));
    *out = {storage.get(), bytes(storage->detail), storage->skin, static_cast<uint8_t>(storage->outcome)};
    storage.release();
} catch (...) {}
void* create_shader(void* pack, void* raw, void* device, void* reflection, bool attach, bool* refused) noexcept {
    const auto& api = static_cast<RuntimeShaders*>(pack)->api();
    if (attach && api.attach_d3d11(device) != RSF_PATCH_OK) { *refused = true; return nullptr; }
    void* shader{};
    const auto& request = static_cast<RequestStorage*>(raw)->request;
    auto& buffer = *static_cast<Prepared*>(reflection);
    const auto result = api.create_d3d11(device, request.target.c_str(), {buffer.variant.data(), buffer.variant.size()}, &shader);
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
    [](void* module, uint32_t size, const void* profile) -> const char* {
        const char* failed = "unknown";
        return guards_match(static_cast<std::byte*>(module), size, *static_cast<const BuildProfile*>(profile)->motion, &failed)
            ? nullptr : failed;
    },
    geometry::geometry_hooks_active,
    [](bool enabled) {
        if (enabled) geometry::geometry_set_sink({&retirements, &geometry::RetirementQueue::sink});
        geometry::geometry_set_flush_callback(enabled ? flush : nullptr);
    },
    geometry::geometry_enable_foliage_identity, install_hooks, [] { return hooks.disable(); }, [] { return hooks.created(); },
    install_draw_hooks, window_facts, target_facts, backend, hw_context, hw_device,
    [](void* c, void* rtv) {
        static constexpr float zero[4]{};
        static_cast<ID3D11DeviceContext*>(c)->ClearRenderTargetView(static_cast<ID3D11RenderTargetView*>(rtv), zero);
    },
    bound_shaders, set_shaders,
    [](const rsf_anomaly_motion_device_view* view, void* c, Redraw draw, void* user) {
        return static_cast<uint8_t>(MotionDevice::invalidate(*view, static_cast<ID3D11DeviceContext*>(c),
            [&](ID3D11DeviceContext* context) { draw(user, context); }));
    },
    camera, [](float (*world)[16]) { return geometry::read_world_matrix(*world); },
    [](Retire apply, void* user) -> size_t {
        struct Sink { Retire apply; void* user; } sink{apply, user};
        return retirements.drain([](void* u, temporal::InstanceKey key) {
            const auto& s = *static_cast<const Sink*>(u);
            const auto native = geometry::geometry_key(key);
            s.apply(s.user, &native);
        }, &sink);
    },
    geometry::read_vertex_constants, geometry::write_vertex_constants,
    [](void* raw, MotionDrawState* out) {
        D3D11_VIEWPORT viewport{};
        UINT count = 1;
        static_cast<ID3D11DeviceContext*>(raw)->RSGetViewports(&count, &viewport);
        const auto jitter = sr::current_jitter_uv();
        *out = {{viewport.TopLeftX, viewport.TopLeftY, viewport.Width, viewport.Height},
            {jitter.current_u, jitter.current_v, jitter.previous_u, jitter.previous_v}};
    },
    open_pack, [](void* pack) { delete static_cast<RuntimeShaders*>(pack); },
    [](void* pack) { set_shader_runtime(static_cast<RuntimeShaders*>(pack)->api()); },
    [](void* pack, void* device) { static_cast<RuntimeShaders*>(pack)->api().attach_d3d11(device); },
    [](void* pack, const wchar_t* path) { return static_cast<RuntimeShaders*>(pack)->api().dump(path); },
    [](void* pack, PatchCounters* out) {
        rsf_patch_counters raw{sizeof(raw), RSF_SHADER_PATCH_VERSION};
        static_cast<RuntimeShaders*>(pack)->api().counters(&raw);
        *out = {raw.intercepted, raw.built, raw.cache_hits, raw.refused, {}};
        std::copy(std::begin(raw.refusals), std::end(raw.refusals), out->refusals);
    },
    decode_request, [](void* request) { delete static_cast<RequestStorage*>(request); },
    copy_bytecode,
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

static_assert(sizeof(WindowFacts) == 56 && sizeof(DrawFacts) == 32);
static_assert(sizeof(ShaderObjects) == 40 && sizeof(ShaderResult) == 32 && sizeof(PackInfo) == 8);
static_assert(sizeof(FoliageLayout) == 108 && offsetof(FoliageLayout, xform) == 4 && offsetof(FoliageLayout, usable) == 104);
static_assert(sizeof(MotionPlatform) == 336 && offsetof(MotionPlatform, guards) == 40);
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
