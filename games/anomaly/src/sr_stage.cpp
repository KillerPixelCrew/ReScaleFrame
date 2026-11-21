// SPDX-License-Identifier: MIT
// Native leaves of the SR stage driver. sr_stage.rs owns the lifecycle, view admission, the H1 and seam ordering,
// jitter and camera publication, capture and burst bookkeeping, the configuration (INI path, vendor directories, live
// panel options) and status (sr_stage_platform.h). This file keeps the guarded engine reads, the stage hooks and
// trampolines, the SwapController with its D3D11 host, the vendor file probe and the TemporalDriver facade.
#include "sr_stage.h"
#include "sr_stage_platform.h"
#include "engine_slots.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include "native_observer.h"
#include "motion_stage.h"
#include "runtime_module.h"
#include "sr_d3d11.h"
#include "build_profile.h"
#include "sr_jitter.h"
#include "sr_plan.h"
#include "sr_swap.h"
#include "sr_reconstruction.h"
#include "sr_temporal.h"
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/game_frame.h>
#include <rescaleframe/overlay_api.h>
#include <rescaleframe/texture_dump.h>
#include <windows.h>
#include <algorithm>
#include <cstdio>
#include <cmath>
#include <cstring>
#include <cwchar>
#include <string_view>
#include <tuple>

namespace rsf { uint32_t texture_pixel_bytes(DXGI_FORMAT format); }

extern "C" {
int32_t rsf_anomaly_sr_ini_int(const void*, const char*, int32_t) noexcept;
uint32_t rsf_anomaly_sr_ini_text(const void*, const char*, const char*, uint16_t*, uint32_t) noexcept;
void rsf_anomaly_sr_persist_int(const char*, int32_t) noexcept;
void rsf_anomaly_sr_persist_text(const char*, const char*) noexcept;
void rsf_anomaly_sr_stage_register(const rsf_anomaly_sr_stage_platform*) noexcept;
void rsf_anomaly_sr_stage_read_options(const void*, rsf_anomaly_sr_stage_config*) noexcept;
float rsf_anomaly_sr_stage_read_scale(const void*, bool*) noexcept;
float rsf_anomaly_sr_stage_read_mip_bias(const void*, bool*) noexcept;
bool rsf_anomaly_sr_stage_configure_ini(const wchar_t*, bool, rsf_anomaly_sr_log, void*) noexcept;
void rsf_anomaly_sr_stage_set_scale(float) noexcept;
bool rsf_anomaly_sr_stage_install(void*, uint32_t, uint32_t, const void*) noexcept;
bool rsf_anomaly_sr_stage_quiesce() noexcept;
bool rsf_anomaly_sr_stage_active() noexcept;
void rsf_anomaly_sr_stage_prepare(void*) noexcept;
void rsf_anomaly_sr_stage_prepared() noexcept;
void rsf_anomaly_sr_stage_seam(void*) noexcept;
void rsf_anomaly_sr_stage_combine_end() noexcept;
bool rsf_anomaly_sr_stage_sun_begin() noexcept;
void rsf_anomaly_sr_stage_sun_end(bool) noexcept;
void rsf_anomaly_sr_stage_position_setup(void*) noexcept;
void rsf_anomaly_sr_stage_rm_near() noexcept;
void rsf_anomaly_sr_stage_frame_end() noexcept;
bool rsf_anomaly_sr_stage_upscale(void*, const rsf_anomaly_sr_seam*) noexcept;
bool rsf_anomaly_sr_stage_round_trip(void*, const rsf_anomaly_sr_seam*) noexcept;
void rsf_anomaly_sr_stage_report(rsf_anomaly_sr_log, void*) noexcept;
void rsf_anomaly_sr_stage_jitter(rsf_anomaly_sr_jitter_state*) noexcept;
void rsf_anomaly_sr_stage_jitter_uv(rsf_anomaly_sr_jitter_uv*) noexcept;
}

namespace rsf::anomaly::sr {
// Borrowed SR guard facts for the selected build. Each array pointer borrows the full profile array.
struct SrGuardProfile {
    const NativeSite* scene_prepare;
    const NativeSite* combine;
    const NativeSite* sunshafts;
    const NativeSite* blur;
    const NativeSite* sun_cascades;
    const NativeSite* position_setup;
    const NativeSite* set_projection;
    const NativeSite* set_mip_bias;
    const UniqueGuard* unique;
    size_t unique_count;
    const NativeSite* call_sites;
    const FirstPersonSite* first_person;
    const UniqueGuard* reconstruction;
    const char* ssao_code;
    uint32_t ssao_rva;
    uint32_t rm_near_rva;
};
static_assert(sizeof(NativeSite) == 32 && offsetof(NativeSite, rva) == 8 && offsetof(NativeSite, expected) == 12);
static_assert(sizeof(UniqueGuard) == 24 && offsetof(UniqueGuard, rva) == 8 && offsetof(UniqueGuard, expected_hex) == 16);
static_assert(sizeof(FirstPersonSite) == 24 && offsetof(FirstPersonSite, set_p_call) == 12 &&
    offsetof(FirstPersonSite, rm_near_call) == 16);
static_assert(sizeof(SrGuardProfile) == 120 && offsetof(SrGuardProfile, unique) == 64 &&
    offsetof(SrGuardProfile, call_sites) == 80 && offsetof(SrGuardProfile, ssao_rva) == 112);
extern "C" const char* rsf_anomaly_sr_guard_failure(const rsf_anomaly_guard_image*,
    const SrGuardProfile*, bool world_window, bool temporal) noexcept;

static_assert(static_cast<std::size_t>(Skip::Count) == RSF_ANOMALY_SR_SKIPS &&
    static_cast<std::size_t>(Restore::Count) == RSF_ANOMALY_SR_RESTORES);
static_assert(static_cast<uint8_t>(Refusal::Prepare) == 2 && static_cast<uint8_t>(Refusal::Reinsert) == 5);
static_assert(DXGI_FORMAT_R16G16B16A16_FLOAT == 10 && RSF_DUMP_VIEW_HDR == 3 && RSF_DUMP_VIEW_DEPTH == 4 &&
    RSF_DUMP_VIEW_DENSE_MOTION == 7 && RSF_PHASE_RENDER_SUBMIT == 2 && RSF_SCREEN_UNKNOWN == 0);
namespace {
// ---- engine memory ------------------------------------------------------------------------------------------
std::byte* image{};
// The exact build install() was given (build_profile.h). Set together with `image`; until then every read fails on the
// null image, and the empty tables below are never dereferenced into engine memory.
const BuildProfile* build{};
const SrProfile& srp() noexcept { static constexpr SrProfile none{}; return build ? *build->sr : none; }
const NativeProfile& np() noexcept { static constexpr NativeProfile none{}; return build ? *build->native : none; }
template<class T> bool read_global(uint32_t rva, T& value) noexcept { return image && guarded_copy(&value, image + rva, sizeof(T)); }

// ---- Stage 3 engine calls (byte-guarded at install) --------------------------------------------------------------
// R_xforms::set_P(this = &RCache.xforms, rdx = const Fmatrix*) and dx10SamplerStateCache::SetMipLODBias(this =
// &SSManager, xmm1 = float), both x64 __fastcall (sr_profile.h sr_function_sites).
using SetProjection = void(__fastcall*)(void* xforms, const void* matrix);
using SetMipBias = void(__fastcall*)(void* cache, float bias);
SetProjection set_projection_fn{};
SetMipBias set_mip_bias_fn{};
bool call_set_projection(const void* matrix) noexcept {
    if (!set_projection_fn || !image) return false;
    __try { set_projection_fn(image + srp().backend_rva + backend_xforms_offset, matrix); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
bool call_set_mip_bias(float value) noexcept {
    if (!set_mip_bias_fn || !image) return false;
    __try { set_mip_bias_fn(image + srp().sampler_cache_rva, value); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

using GetConstantBuffer = std::byte*(__fastcall*)(void*, void*, uint32_t);
GetConstantBuffer get_constant_buffer_fn{};
struct EnginePositionConstants final : PositionConstantHost {
    bool read(const void* p, void* out, std::size_t size) noexcept override { return guarded_copy(out, p, size); }
    bool write(void* p, const void* data, std::size_t size) noexcept override { return guarded_copy(p, data, size); }
    std::byte* buffer(void* constant, uint32_t stage) noexcept override {
        if (!get_constant_buffer_fn || !image) return nullptr;
        __try { return get_constant_buffer_fn(image + srp().backend_rva + backend_constants_offset, constant, stage); }
        __except (EXCEPTION_EXECUTE_HANDLER) { return nullptr; }
    }
};
EnginePositionConstants position_constants;

// Stage 3 burst readback leaves. sr_stage/staging.rs admits the context, source and device and describes the
// staging texture (USAGE_STAGING, CPU_ACCESS_READ, one mip and layer); these keep the typed context calls.
static_assert(D3D11_DEVICE_CONTEXT_IMMEDIATE == 0 && D3D11_USAGE_STAGING == 3 && D3D11_CPU_ACCESS_READ == 0x20000);
static_assert(sizeof(rsf_anomaly_sr_frame_reads) == 168 && offsetof(rsf_anomaly_sr_frame_reads, render_view) == 152);
static_assert(static_cast<uint32_t>(temporal::ViewId::Scope) == 1 && dx10_msaa_bit == (1u << 14));
static_assert(sizeof(rsf_anomaly_sr_stage_platform) == 640 &&
    offsetof(rsf_anomaly_sr_stage_platform, immediate_context) == 280 &&
    offsetof(rsf_anomaly_sr_stage_platform, pixel_bytes) == 328 && offsetof(rsf_anomaly_sr_stage_platform, bilinear) == 336);
// Predication is cleared for the copy and restored afterwards, as the burst always did.
void copy_subresource(void* context, void* destination, uint32_t destination_subresource, void* source,
    uint32_t source_subresource) noexcept {
    auto* ctx = static_cast<ID3D11DeviceContext*>(context);
    ComPtr<ID3D11Predicate> predicate;
    BOOL predicate_value{};
    ctx->GetPredication(&predicate, &predicate_value);
    ctx->SetPredication(nullptr, FALSE);
    ctx->CopySubresourceRegion(static_cast<ID3D11Resource*>(destination), destination_subresource, 0, 0, 0,
        static_cast<ID3D11Resource*>(source), source_subresource, nullptr);
    ctx->SetPredication(predicate.Get(), predicate_value);
}
bool map_read(void* context, void* texture, uint32_t subresource, const void** data, uint32_t* row_pitch) noexcept {
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(static_cast<ID3D11DeviceContext*>(context)->Map(static_cast<ID3D11Resource*>(texture), subresource,
            D3D11_MAP_READ, 0, &mapped))) return false;
    *data = mapped.pData;
    *row_pitch = mapped.RowPitch;
    return true;
}

uint32_t twin_refs(void* raw) {
    auto* texture = static_cast<ID3D11Texture2D*>(raw);
    texture->AddRef();
    return texture->Release() - 1;
}

// ---- the D3D11/engine host behind the swap controller --------------------------------------------------------
struct TwinLog {
    rsf_anomaly_sr_log callback{};
    void* user{};
    void say(const char* message) const noexcept {
        if (callback) try { callback(user, message); } catch (...) {}
    }
};

struct Host final : SwapHost {
    rsf_sr_stage_resources* resources{};
    const TwinLog* twin_log{}; // borrowed only while controller.prepare() runs
    // While controller.seam() runs: the Rust scope lent by the seam call, and the seam being closed.
    void* cookie{};
    const UpscaleViews* seam_views{};
    const rsf_sr_draw_views* trip_views{};
    Host() = default;
    Host(const Host&) = delete;
    Host& operator=(const Host&) = delete;
    ~Host() { rsf_anomaly_sr_stage_drop_resources(resources); }
    const rsf_sr_pass* pass() const noexcept { return rsf_anomaly_sr_stage_pass(resources); }
    void* depth_texture() const noexcept { return rsf_anomaly_sr_stage_depth(resources); }
    bool read(const void* address, void* output, std::size_t size) noexcept override { return guarded_copy(output, address, size); }
    bool write(void* address, const void* input, std::size_t size) noexcept override { return guarded_copy(address, input, size); }
    ID3D11Device* device() noexcept { ID3D11Device* value{}; return read_global(srp().hw_device_rva, value) ? value : nullptr; }
    ID3D11DeviceContext* context() noexcept { ID3D11DeviceContext* value{}; return read_global(srp().hw_context_rva, value) ? value : nullptr; }
    MakeResult make_twins(const TwinRequest& request, TwinSet& out) noexcept override {
        const auto log = twin_log ? *twin_log : TwinLog{};
        auto* d3d = device();
        if (!d3d) { log.say("Anomaly SR stage: HW.pDevice is null; twins not created."); return MakeResult::Failed; }
        rsf_sr_twin_batch_info info{};
        const auto result = static_cast<MakeResult>(
            rsf_anomaly_sr_stage_make_twins(&resources, &d3d11_platform(), d3d, &request, &out, &info, log.callback, log.user));
        if (info.pass_refused) {
            log.say(request.stage == 1 ? "Anomaly SR stage: round-trip shader could not be created."
                : "Anomaly SR stage: upscale shader could not be created.");
            return result;
        }
        char text[260];
        if (result != MakeResult::Created) {
            const auto error = twin_error_name(static_cast<TwinError>(info.cause));
            if (info.failed_index < slot_count) {
                const auto i = info.failed_index;
                const auto& spec = request.spec[i];
                std::snprintf(text, sizeof(text), "Anomaly SR stage: twin of %s refused (%s) at %ux%u; this frame stays stock.",
                    sr_world_targets[i].name, error, spec.width, spec.height);
            } else if (info.failed_index == slot_count) {
                std::snprintf(text, sizeof(text), "Anomaly SR stage: depth twin refused (%s) at %ux%u.",
                    error, request.render_width, request.render_height);
            } else {
                std::snprintf(text, sizeof(text), "Anomaly SR stage: HW.pBaseRT stand-in refused (%s) at %ux%u.",
                    error, request.render_width, request.render_height);
            }
            log.say(text);
            return result;
        }
        if (request.stage == 1)
            std::snprintf(text, sizeof(text), "Anomaly SR stage: FP16 twins created for generic0/generic_temp at %ux%u (uav=%d/%d).",
                out.targets[0].width, out.targets[0].height, out.targets[0].uav ? 1 : 0, out.targets[1].uav ? 1 : 0);
        else
            std::snprintf(text, sizeof(text),
                "Anomaly SR stage 2: %u world twins, a depth twin and a HW.pBaseRT stand-in created at render %ux%u for output %ux%u (%.1f MiB).",
                info.made, request.render_width, request.render_height, request.output_width, request.output_height,
                double(info.bytes) / (1024.0 * 1024.0));
        log.say(text);
        return MakeResult::Created;
    }
    void drop_twins() noexcept override {
        // Reference counts after our own views are gone. The engine's surface_get() before its generic_temp copy
        // takes references it never releases (stock leaks them on its own targets too), so this is diagnostic only.
        rsf_anomaly_sr_stage_retire_twins(resources, twin_refs);
    }
    // Stage 2 and later: the Rust seam pass decides inputs, captures and fallback; the leaves below act on these views.
    bool upscale(const UpscaleInputs& in) noexcept override {
        if (!in.generic0_twin || !in.position_twin || !in.generic1_twin || !in.generic0 || !in.position || !in.generic1) return false;
        const UpscaleViews views{static_cast<ID3D11ShaderResourceView*>(in.generic0_twin->srv),
            static_cast<ID3D11ShaderResourceView*>(in.position_twin->srv), static_cast<ID3D11ShaderResourceView*>(in.generic1_twin->srv),
            static_cast<ID3D11ShaderResourceView*>(in.depth_srv), in.render_width, in.render_height,
            static_cast<ID3D11RenderTargetView*>(in.generic0->rtv), static_cast<ID3D11RenderTargetView*>(in.position->rtv),
            static_cast<ID3D11RenderTargetView*>(in.generic1->rtv), static_cast<ID3D11DepthStencilView*>(in.engine_dsv),
            in.output_width, in.output_height};
        const rsf_anomaly_sr_seam seam{in.generic0_twin->surface, in.generic0->surface, in.render_width, in.render_height,
            in.output_width, in.output_height};
        seam_views = &views;
        const bool ok = rsf_anomaly_sr_stage_upscale(cookie, &seam);
        seam_views = nullptr;
        return ok;
    }
    void set_viewport(uint32_t width, uint32_t height) noexcept override { set_full_viewport(context(), width, height); }
    bool upload_projection(const void* matrix) noexcept override { return call_set_projection(matrix); }
    bool set_mip_bias(float value) noexcept override { return call_set_mip_bias(value); }
    bool correct_position_constant(void* constant, float x, float y) noexcept override {
        return sr::correct_position_constant(position_constants, constant, x, y);
    }
    bool round_trip(const TargetFields& twin, const TargetFields& engine) noexcept override {
        const rsf_anomaly_sr_seam seam{twin.surface, engine.surface, twin.width, twin.height, engine.width, engine.height};
        const auto views = round_trip_views(twin.srv, twin.width, twin.height, engine.rtv, engine.width, engine.height);
        trip_views = &views;
        const bool ok = rsf_anomaly_sr_stage_round_trip(cookie, &seam);
        trip_views = nullptr;
        return ok;
    }
};

// ---- process state --------------------------------------------------------------------------------------------
// The leaf objects below are used under the Rust stage lock (the callbacks run inside its calls).
Host host;
SwapController controller{host};
HookSet hooks;
TemporalDriver driver;

std::array<void*, 5> originals{};
using Unary = void(__fastcall*)(void*);

// ---- conversions between the native types and the Rust boundary --------------------------------------------------
JitterState to_native(const rsf_anomaly_sr_jitter_state& s) noexcept {
    JitterState out{};
    out.frame = s.frame;
    out.sequence_index = s.sequence_index;
    out.phase_count = s.phase_count;
    out.current_x = s.current_x; out.current_y = s.current_y;
    out.previous_x = s.previous_x; out.previous_y = s.previous_y;
    out.render_width = s.render_width; out.render_height = s.render_height;
    out.previous_render_width = s.previous_render_width; out.previous_render_height = s.previous_render_height;
    out.scope = s.scope; out.applied = s.applied; out.previous_applied = s.previous_applied; out.reset = s.reset;
    return out;
}
SrSettings to_native(const rsf_anomaly_sr_settings& s) noexcept { return {s.enabled, s.backend, s.quality, s.preset}; }
// ---- guards and hooks -----------------------------------------------------------------------------------------------
// The original can re-enter draw callbacks, so the stage lock is held only while changing the camera.
// Scope cleanup also restores the camera if an engine C++ exception leaves the original.
struct SunCameraScope {
    bool suspended{};
    SunCameraScope() noexcept : suspended(rsf_anomaly_sr_stage_sun_begin()) {}
    ~SunCameraScope() { rsf_anomaly_sr_stage_sun_end(suspended); }
};
void __fastcall prepare_hook(void* self) {
    rsf_anomaly_sr_stage_prepare(self);
    reinterpret_cast<Unary>(originals[0])(self);
    rsf_anomaly_sr_stage_prepared();   // Stage 2: the viewport now describes the render-size targets
}
void __fastcall position_setup_hook(void* self, void* constant) {
    const auto original = rsf_anomaly_slot_original(RSF_ANOMALY_POSITION_SLOT);
    reinterpret_cast<void(__fastcall*)(void*, void*)>(original)(self, constant);
    if (rsf_anomaly_position_slot_active()) rsf_anomaly_sr_stage_position_setup(constant);
}
// cl_pos_decompress_params::setup is only ever reached through the binder's vtable (R_constant_setup::setup, slot 0).
// The AVX build's body has a VEX instruction (vmovss) seven bytes in, which MinHook's decoder cannot relocate
// (MH_ERROR_UNSUPPORTED_FUNCTION on the real executable; the SSE build is fine). So this site swaps the vtable slot on
// both builds instead of detouring the body. The slot must hold the profiled function, and the function its guard.
bool install_position_slot(std::byte* module) noexcept {
    const auto& site = srp().sr_function_sites[site_position_setup];
    const auto checked = guard_image(module, build->image_size());
    return matches(checked, site) && rsf_anomaly_slot_install(RSF_ANOMALY_POSITION_SLOT, &checked,
        srp().position_setup_vtable_rva, site.rva, 0, reinterpret_cast<void*>(&position_setup_hook), compare_slot);
}
void __fastcall sun_cascades_hook(void* self) {
    SunCameraScope camera;
    reinterpret_cast<Unary>(originals[4])(self);
}
void __fastcall sunshafts_hook(void* self) { rsf_anomaly_sr_stage_seam(self); reinterpret_cast<Unary>(originals[2])(self); }
void __fastcall blur_hook(void* self) { rsf_anomaly_sr_stage_seam(self); reinterpret_cast<Unary>(originals[3])(self); }
void __fastcall combine_hook(void* self) {
    reinterpret_cast<Unary>(originals[1])(self);
    rsf_anomaly_sr_stage_combine_end();
}
void rm_near_callback(void*) noexcept { rsf_anomaly_sr_stage_rm_near(); }

bool install_hooks() noexcept {
    if (hooks.created()) return hooks.enable();
    if (!build || !image) return false;
    const auto site = [](std::size_t index, void* detour, std::size_t original) {
        const auto& s = srp().sr_function_sites[index];
        return HookSite{s.name, s.rva, s.expected, detour, &originals[original]};
    };
    const std::array<HookSite, 5> sites{{site(site_phase_scene_prepare, reinterpret_cast<void*>(&prepare_hook), 0),
        site(site_phase_combine, reinterpret_cast<void*>(&combine_hook), 1),
        site(site_phase_sunshafts, reinterpret_cast<void*>(&sunshafts_hook), 2),
        site(site_phase_blur, reinterpret_cast<void*>(&blur_hook), 3),
        site(site_sun_cascades, reinterpret_cast<void*>(&sun_cascades_hook), 4)}};
    return hooks.install(image, build->image_size(), sites);
}

// ---- the leaf table ---------------------------------------------------------------------------------------------
const rsf_anomaly_sr_stage_platform platform{
    .render_target = []() -> void* {
        std::byte* target{};
        return read_global(srp().render_implementation_rva + render_target_offset, target) ? target : nullptr;
    },
    .frame_facts = [](bool world_window, bool temporal, rsf_anomaly_sr_frame_reads* reads) {
        *reads = {};
        if (!image || !build) return;
        auto* out = &reads->facts;
        out->backend = image + srp().backend_rva;
        read_global(srp().hw_device_rva, out->device);
        NativeObservation observation{sizeof(observation), observation_abi};
        if (rsf_anomaly_observation(&observation) == RSF_OK) out->generation = observation.resets;
        read_global(np().device_width_rva, out->device_width);
        read_global(np().device_height_rva, out->device_height);
        const auto view = geometry::current_render_view(image, *build);
        reads->view_read = view.has_value();
        if (view) reads->render_view = static_cast<uint32_t>(*view);
        reads->options_read = read_global(srp().render_options_word3_rva, reads->options_word3);
        if (world_window) {
            out->device_size = image + np().device_width_rva;
            out->base_zb = image + srp().hw_base_zb_rva;
            out->base_rt = image + srp().hw_base_rt_rva;
            out->ssao_viewport = image + srp().ssao_viewport_rva;
            out->ssao_viewport_guard = image + srp().ssao_viewport_guard_rva;
        }
        if (temporal) {
            out->device_view = image + srp().device_view_rva;
            out->device_project = image + srp().device_project_rva;
            out->device_full_transform = image + srp().device_full_transform_rva;
            out->sampler_bias = image + srp().sampler_cache_rva + sampler_mip_lod_bias_offset;
        }
    },
    .loading_registered = [](uint8_t* registered) { return read_global(srp().load_screen_registered_rva, *registered); },
    .precache_frame = [](uint32_t* frame) { return read_global(srp().device_precache_frame_rva, *frame); },
    .engine_frame = [] { uint32_t frame{}; read_global(np().device_frame_rva, frame); return frame; },
    .device_size = [](uint32_t* width, uint32_t* height) {
        read_global(np().device_width_rva, *width);
        read_global(np().device_height_rva, *height);
    },
    .read_camera = [](rsf_anomaly_sr_camera* out) {
        return read_global(srp().device_view_rva, out->view) && read_global(srp().device_project_rva, out->projection);
    },
    .seam_surface = [](void* target) -> void* {
        std::byte* crt{};
        void* surface{};
        if (guarded_copy(&crt, static_cast<std::byte*>(target) + 0x3a8, sizeof(crt)) && crt &&
            guarded_copy(&surface, crt + crt_surface_offset, sizeof(surface)))
            return surface;
        return nullptr;
    },
    .back_buffer = []() -> void* {
        ID3D11RenderTargetView* base{};
        if (!read_global(srp().hw_base_rt_rva, base) || !base) return nullptr;
        ID3D11Resource* back{};
        base->GetResource(&back);
        return back;
    },
    .context = []() -> void* { return host.context(); },
    .device = []() -> void* { return host.device(); },
    .now = [](int64_t* counter, int64_t* frequency) {
        LARGE_INTEGER now{}, rate{};
        QueryPerformanceCounter(&now);
        QueryPerformanceFrequency(&rate);
        *counter = now.QuadPart;
        *frequency = rate.QuadPart;
    },
    .geometry_active = [] { return geometry::geometry_hooks_active(); },
    .build_sha256 = []() -> const char* { return build ? build->sha256 : ""; },
    .close_stale = [] { controller.close_stale(); },
    .prepare = [](const rsf_anomaly_sr_frame_facts* in, rsf_anomaly_sr_log log, void* user) {
        FrameFacts facts{};
        facts.render_target = static_cast<std::byte*>(in->render_target);
        facts.backend = static_cast<std::byte*>(in->backend);
        facts.device = in->device;
        facts.generation = in->generation;
        facts.device_width = in->device_width;
        facts.device_height = in->device_height;
        facts.scope = in->scope;
        facts.msaa = in->msaa;
        facts.stage = in->stage;
        facts.render_width = in->render_width;
        facts.render_height = in->render_height;
        facts.device_size = static_cast<std::byte*>(in->device_size);
        facts.base_zb = static_cast<std::byte*>(in->base_zb);
        facts.base_rt = static_cast<std::byte*>(in->base_rt);
        facts.ssao_viewport = static_cast<std::byte*>(in->ssao_viewport);
        facts.ssao_viewport_guard = static_cast<std::byte*>(in->ssao_viewport_guard);
        facts.jitter = in->jitter;
        facts.jitter_x = in->jitter_x;
        facts.jitter_y = in->jitter_y;
        facts.device_view = static_cast<std::byte*>(in->device_view);
        facts.device_project = static_cast<std::byte*>(in->device_project);
        facts.device_full_transform = static_cast<std::byte*>(in->device_full_transform);
        facts.mip_bias = in->mip_bias;
        facts.mip_bias_delta = in->mip_bias_delta;
        facts.sampler_bias = static_cast<std::byte*>(in->sampler_bias);
        const TwinLog call_log{log, user};
        const auto* previous = host.twin_log;
        host.twin_log = &call_log;
        const bool prepared = controller.prepare(facts);
        host.twin_log = previous;
        return prepared;
    },
    .prepared = [] { controller.prepared(); },
    .seam = [](void* cookie) {
        host.cookie = cookie;
        const bool closed = controller.seam();
        host.cookie = nullptr;
        return closed;
    },
    .combine_end = [] { controller.combine_end(); },
    .frame_end = [] { controller.frame_end(); },
    .rm_near = [] { controller.rm_near(); },
    .sun_begin = [] { return controller.sun_cascades_begin(); },
    .sun_end = [](bool suspended) { controller.sun_cascades_end(suspended); },
    .position_setup = [](void* constant) { controller.position_constant_setup(constant); },
    .shutdown = [] { controller.shutdown(); },
    .set_swap_enabled = [](bool enabled) { controller.set_swap_enabled(enabled); },
    .controller = [](rsf_anomaly_sr_controller* out) {
        *out = {controller.window_open(), controller.refused(), controller.jitter_applied(), controller.mip_bias_applied(),
            controller.stage(), controller.render_width(), controller.render_height(), controller.window_mip_bias(),
            controller.saved_mip_bias(), controller.twin_generation(), controller.refusal()};
    },
    .swap_counters = [](rsf_anomaly_sr_swap_counters* out) {
        const auto& c = controller.counters();
        *out = {c.prepares, c.eligible, c.swapped, c.seams, c.seams_without_window, c.round_trips, c.round_trip_failures,
            c.combine_ends, c.rm_near_in_window, c.twin_generations, c.twin_drops, c.foreign_changes, c.write_faults,
            c.allocation_failures, c.viewports, c.ssao_viewport_swaps, c.jittered, c.jitter_invalid, c.fp_jittered,
            c.fp_already, c.fp_foreign, c.projection_restores, c.mip_applied, c.mip_restored, c.mip_unchanged, c.sun_calls,
            c.sun_unjittered, c.sun_restored, c.sun_refused, c.position_corrected, c.position_stock, c.position_refused};
        std::copy(std::begin(c.skipped), std::end(c.skipped), out->skipped);
        std::copy(std::begin(c.restores), std::end(c.restores), out->restores);
    },
    .timer_retain_device = [](void* device) {
        static_cast<ID3D11Device*>(device)->AddRef();
        return device;
    },
    .timer_create_query = [](void* device, bool disjoint) -> void* {
        const D3D11_QUERY_DESC desc{disjoint ? D3D11_QUERY_TIMESTAMP_DISJOINT : D3D11_QUERY_TIMESTAMP, 0};
        ComPtr<ID3D11Query> query;
        return SUCCEEDED(static_cast<ID3D11Device*>(device)->CreateQuery(&desc, &query)) ? query.Detach() : nullptr;
    },
    .timer_issue = [](void* context, void* query, bool begin) {
        auto* ctx = static_cast<ID3D11DeviceContext*>(context);
        auto* timestamp = static_cast<ID3D11Query*>(query);
        if (begin) ctx->Begin(timestamp); else ctx->End(timestamp);
    },
    .timer_clock = [](void* context, void* query, uint64_t* frequency, bool* disjoint) {
        D3D11_QUERY_DATA_TIMESTAMP_DISJOINT clock{};
        if (static_cast<ID3D11DeviceContext*>(context)->GetData(static_cast<ID3D11Query*>(query),
                &clock, sizeof(clock), D3D11_ASYNC_GETDATA_DONOTFLUSH) != S_OK) return false;
        *frequency = clock.Frequency;
        *disjoint = clock.Disjoint != FALSE;
        return true;
    },
    .timer_timestamp = [](void* context, void* query, uint64_t* ticks) {
        return static_cast<ID3D11DeviceContext*>(context)->GetData(static_cast<ID3D11Query*>(query),
            ticks, sizeof(*ticks), D3D11_ASYNC_GETDATA_DONOTFLUSH) == S_OK;
    },
    .host_stats = [](uint64_t* bytes, uint64_t* refs) { rsf_anomaly_sr_stage_resource_stats(host.resources, bytes, refs); },
    .host_reset = [] { rsf_anomaly_sr_stage_reset_pass(host.resources); },
    .immediate_context = [](void* context) {
        return static_cast<ID3D11DeviceContext*>(context)->GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE;
    },
    .context_device = [](void* context) -> void* {
        ID3D11Device* device{};
        static_cast<ID3D11DeviceContext*>(context)->GetDevice(&device);
        return device;
    },
    .copy_subresource = copy_subresource,
    .release = [](void* object) { if (object) static_cast<IUnknown*>(object)->Release(); },
    .map_read = map_read,
    .unmap = [](void* context, void* texture, uint32_t subresource) {
        static_cast<ID3D11DeviceContext*>(context)->Unmap(static_cast<ID3D11Resource*>(texture), subresource);
    },
    .pixel_bytes = [](uint32_t format) { return rsf::texture_pixel_bytes(static_cast<DXGI_FORMAT>(format)); },
    .bilinear = [] {
        if (!host.seam_views) return false;
        const auto views = upscale_views(*host.seam_views);
        return rsf_anomaly_sr_run_pass(host.pass(), host.context(), RSF_SR_UPSCALE, &views) != 0;
    },
    .round_trip = [] {
        return host.trip_views && rsf_anomaly_sr_run_pass(host.pass(), host.context(), RSF_SR_ROUND_TRIP, host.trip_views) != 0;
    },
    .temporal_ready = [] { return host.seam_views && rsf_anomaly_sr_temporal_ready(host.pass()) && host.depth_texture(); },
    .temporal_platform = []() -> const void* { return &d3d11_platform(); },
    .motion_target = [](rsf_anomaly_sr_motion_target* out) {
        const auto target = motion::frame_target();
        *out = {target.texture, target.written, target.main, target.frame, target.width, target.height};
        return target.texture != nullptr;
    },
    .create_motion_view = rsf_d3d11_create_shader_view,
    .seam_prepare = [](void* motion_view, void* colour_target, void* motion_target, bool first_person_zero) {
        if (!host.seam_views) return false;
        const auto& views = *host.seam_views;
        const PrepareViews prepare{views.colour, static_cast<ID3D11ShaderResourceView*>(motion_view), views.depth,
            views.source_width, views.source_height, static_cast<ID3D11RenderTargetView*>(colour_target),
            static_cast<ID3D11RenderTargetView*>(motion_target), first_person_depth_limit,
            first_person_zero};
        const auto input = prepare_views(prepare);
        return rsf_anomaly_sr_run_pass(host.pass(), host.context(), RSF_SR_PREPARE, &input) != 0;
    },
    .depth_texture = [] { return host.depth_texture(); },
    .reinsert = [](void* reconstruction) {
        if (!host.seam_views) return false;
        const auto views = upscale_views(*host.seam_views, reconstruction);
        return rsf_anomaly_sr_run_pass(host.pass(), host.context(), RSF_SR_REINSERT, &views) != 0;
    },
    .driver_bind = [] { return driver.bind(rsf_anomaly_runtime_module()); },
    .driver_enabled = [] { return driver.enabled(); },
    .driver_output_format = [] { return driver.output_format(); },
    .driver_planned = [](uint32_t ow, uint32_t oh, uint32_t* rw, uint32_t* rh) { return driver.planned_render(ow, oh, *rw, *rh); },
    .driver_boundary = [](uint32_t width, uint32_t height) { driver.boundary(host.device(), width, height); },
    .driver_evaluate = [](rsf_anomaly_sr_pass_inputs* in, rsf_camera_frame* camera, uint32_t* backend) {
        return driver.evaluate(host.context(), *in, {.sequence = in->frame.reset_sequence, .cut = in->frame.cut}, camera, backend);
    },
    .driver_request_dump = [](const char* prefix) {
        try { return driver.request_dump(prefix); } catch (...) { return false; }
    },
    .driver_count = [](uint8_t reason) { driver.count(static_cast<Refusal>(reason)); },
    .driver_count_bilinear = [] { driver.count_bilinear(); },
    .driver_count_prepared = [](bool missing) { driver.count_prepared(missing); },
    .driver_stop = [] { driver.stop(); },
    .driver_report = [](rsf_anomaly_sr_log emit, void* user) { driver.report(emit, user); },
    .driver_overlay_report = [](void* report) { (void)driver.overlay_report(*static_cast<rsf_overlay_sr_report*>(report)); },
    .driver_request = [](const rsf_anomaly_sr_settings* settings) { driver.request(to_native(*settings)); },
    .verify_guards = [](void* module, uint32_t image_size, const void* selected, bool world_window, bool temporal) -> const char* {
        const auto& profile = *static_cast<const BuildProfile*>(selected)->sr;
        const auto& sites = profile.sr_function_sites;
        static_assert(sr_call_site_count == 6 && sr_first_person_site_count == 6 &&
            std::tuple_size_v<decltype(SrProfile::reconstruction_guards)> == 4);
        const SrGuardProfile guards{
            .scene_prepare = &sites[site_phase_scene_prepare],
            .combine = &sites[site_phase_combine],
            .sunshafts = &sites[site_phase_sunshafts],
            .blur = &sites[site_phase_blur],
            .sun_cascades = &sites[site_sun_cascades],
            .position_setup = &sites[site_position_setup],
            .set_projection = &sites[site_set_p],
            .set_mip_bias = &sites[site_set_mip_lod_bias],
            .unique = profile.sr_unique_guards.data(),
            .unique_count = profile.sr_unique_guards.size(),
            .call_sites = profile.sr_call_sites.data(),
            .first_person = profile.sr_first_person_sites.data(),
            .reconstruction = profile.reconstruction_guards.data(),
            .ssao_code = profile.ssao_set_viewport_code,
            .ssao_rva = profile.ssao_set_viewport_rva,
            .rm_near_rva = sites[site_rm_near].rva,
        };
        const auto checked = guard_image(static_cast<const std::byte*>(module), image_size);
        return rsf_anomaly_sr_guard_failure(&checked, &guards, world_window, temporal);
    },
    .hooks_created = [] { return hooks.created(); },
    .set_engine = [](void* module, const void* selected, bool temporal) {
        build = static_cast<const BuildProfile*>(selected);
        image = static_cast<std::byte*>(module);
        if (!temporal) return;
        get_constant_buffer_fn = reinterpret_cast<GetConstantBuffer>(image + srp().get_constant_buffer_rva);
        set_projection_fn = reinterpret_cast<SetProjection>(image + srp().sr_function_sites[site_set_p].rva);
        set_mip_bias_fn = reinterpret_cast<SetMipBias>(image + srp().sr_function_sites[site_set_mip_lod_bias].rva);
    },
    .install_hooks = [] { return install_hooks(); },
    .install_position_slot = [] { return image && install_position_slot(image); },
    .disable_hooks = [] {
        (void)rsf_anomaly_slot_set(RSF_ANOMALY_POSITION_SLOT, false);
        (void)hooks.disable();
    },
    .set_rm_near = [](bool on) { geometry::geometry_set_rm_near_callback(on ? &rm_near_callback : nullptr); },
    .remove_hooks = [] {
        geometry::geometry_set_rm_near_callback(nullptr);
        const bool restored = rsf_anomaly_slot_set(RSF_ANOMALY_POSITION_SLOT, false);
        const bool disabled = hooks.disable();
        return restored && disabled;
    },
    .ini_int = rsf_anomaly_sr_ini_int,
    .ini_text = rsf_anomaly_sr_ini_text,
    .persist_int = rsf_anomaly_sr_persist_int,
    .persist_text = rsf_anomaly_sr_persist_text,
    .apply_colour_transport = rsf_anomaly_runtime_set_colour_transport,
    .file_exists = [](const uint16_t* path) {
        const DWORD attributes = GetFileAttributesW(reinterpret_cast<const wchar_t*>(path));
        return attributes != INVALID_FILE_ATTRIBUTES && !(attributes & FILE_ATTRIBUTE_DIRECTORY);
    },
    .configured = [](rsf_anomaly_sr_log log, void* user, const rsf_anomaly_sr_driver_config* config) {
        if (config) driver.configure(*config, log, user);
    },
};
void bind_platform() noexcept { rsf_anomaly_sr_stage_register(&platform); }
}

JitterState current_jitter() noexcept {
    rsf_anomaly_sr_jitter_state state{};
    rsf_anomaly_sr_stage_jitter(&state);
    return to_native(state);
}
JitterUv current_jitter_uv() noexcept {
    rsf_anomaly_sr_jitter_uv uv{};
    rsf_anomaly_sr_stage_jitter_uv(&uv);
    return {uv.current_u, uv.current_v, uv.previous_u, uv.previous_v};
}

float read_scale(const wchar_t* ini, bool* defaulted) noexcept {
    bind_platform();
    return rsf_anomaly_sr_stage_read_scale(ini, defaulted);
}
float read_mip_bias(const wchar_t* ini, bool* enabled) noexcept {
    bind_platform();
    return rsf_anomaly_sr_stage_read_mip_bias(ini, enabled);
}

rsf_anomaly_sr_stage_config read_options(const wchar_t* ini) noexcept {
    bind_platform();
    rsf_anomaly_sr_stage_config config{};
    rsf_anomaly_sr_stage_read_options(ini, &config);
    return config;
}
void set_scale(float value) noexcept { rsf_anomaly_sr_stage_set_scale(value); }

bool configure(const wchar_t* ini, bool apply, rsf_game_log_fn log, void* user) noexcept {
    bind_platform();
    return rsf_anomaly_sr_stage_configure_ini(ini, apply, log, user);
}
bool active() noexcept { return rsf_anomaly_sr_stage_active(); }

bool install(std::byte* module, uint32_t image_size, const BuildProfile& selected) noexcept {
    bind_platform();
    return rsf_anomaly_sr_stage_install(module, image_size, selected.image_size(), &selected);
}
bool quiesce() noexcept { return rsf_anomaly_sr_stage_quiesce(); }
void frame_end() noexcept { rsf_anomaly_sr_stage_frame_end(); }
void report(void (*emit)(void*, const char*), void* user) noexcept { rsf_anomaly_sr_stage_report(emit, user); }

}
