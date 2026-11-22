// SPDX-License-Identifier: MIT
// Native leaves of the SR stage driver. sr_stage.rs owns the lifecycle, view admission, the H1 and seam ordering,
// jitter and camera publication, capture and burst bookkeeping, the configuration (INI path, vendor directories, live
// panel options) and status (sr_stage_platform.h). This file keeps the guarded engine reads, the stage hooks and
// trampolines, and the guarded engine and graphics callbacks for the Rust swap owner.
#include "sr_stage.h"
#include "sr_stage_platform.h"
#include "engine_guards.h"
#include "engine_slots.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include "native_memory.h"
#include "stage_entries.h"
#include "native_observer.h"
#include "motion_stage.h"
#include "runtime_module.h"
#include "sr_d3d11.h"
#include "build_profile.h"
#include "sr_jitter.h"
#include "sr_pass.h"
#include "sr_swap_layout.h"
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/game_frame.h>
#include <rescaleframe/overlay_api.h>
#include <rescaleframe/texture_dump.h>
#include <windows.h>
#include <algorithm>
#include <bit>
#include <cstdio>
#include <cmath>
#include <cstring>
#include <cwchar>
#include <string_view>
#include <tuple>

extern "C" {
int32_t rsf_anomaly_sr_ini_int(const void*, const char*, int32_t) noexcept;
uint32_t rsf_anomaly_sr_ini_text(const void*, const char*, const char*, uint16_t*, uint32_t) noexcept;
bool rsf_anomaly_sr_persist_int(const char*, int32_t) noexcept;
bool rsf_anomaly_sr_persist_text(const char*, const char*) noexcept;
void rsf_anomaly_sr_stage_register(const rsf_anomaly_sr_stage_platform*) noexcept;
bool rsf_anomaly_sr_stage_configure_ini(const wchar_t*, bool, rsf_anomaly_sr_log, void*) noexcept;
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
// Constant-buffer fields used after the native binder has written the stock vector.
constexpr uint32_t backend_constants_offset = 0x5c0;
constexpr rsf_anomaly_position_layout position_layout{
    0x10, 0x14, geometry::constant_buffer_size_offset, geometry::constant_buffer_data_offset,
    0x68, geometry::constant_buffer_slots, {1, 2, 8, 16, 32, 64}, {16, 12, 8, 20, 24, 28},
};
const rsf_anomaly_position_platform position_constants{
    nullptr,
    [](void*, uintptr_t address, void* out, size_t size) {
        return guarded_copy(out, reinterpret_cast<const void*>(address), size);
    },
    [](void*, uintptr_t address, const void* in, size_t size) {
        return guarded_copy(reinterpret_cast<void*>(address), in, size);
    },
    [](void*, uintptr_t address, uint32_t stage) -> uintptr_t {
        if (!get_constant_buffer_fn || !image) return 0;
        __try {
            return reinterpret_cast<uintptr_t>(get_constant_buffer_fn(
                image + srp().backend_rva + backend_constants_offset, reinterpret_cast<void*>(address), stage));
        }
        __except (EXCEPTION_EXECUTE_HANDLER) { return 0; }
    },
};

// Stage 3 burst readback leaves. sr_stage/staging.rs admits the context, source and device and describes the
// staging texture (USAGE_STAGING, CPU_ACCESS_READ, one mip and layer); these keep the typed context calls.
static_assert(D3D11_DEVICE_CONTEXT_IMMEDIATE == 0 && D3D11_USAGE_STAGING == 3 && D3D11_CPU_ACCESS_READ == 0x20000);
static_assert(sizeof(rsf_anomaly_sr_frame_reads) == 168 && offsetof(rsf_anomaly_sr_frame_reads, render_view) == 152);
static_assert(static_cast<uint32_t>(temporal::ViewId::Scope) == 1 && dx10_msaa_bit == (1u << 14));
static_assert(sizeof(rsf_anomaly_sr_stage_platform) == 608 &&
    offsetof(rsf_anomaly_sr_stage_platform, immediate_context) == 280 &&
    offsetof(rsf_anomaly_sr_stage_platform, pixel_bytes) == 328 && offsetof(rsf_anomaly_sr_stage_platform, draw_pass) == 336);
static_assert(first_person_depth_limit == 0.02f && RSF_SR_ROUND_TRIP == 0 && RSF_SR_UPSCALE == 1 &&
    RSF_SR_PREPARE == 2 && RSF_SR_REINSERT == 3);
// ---- the D3D11/engine host behind the swap controller --------------------------------------------------------
struct TwinLog {
    rsf_anomaly_sr_log callback{};
    void* user{};
    void say(const char* message) const noexcept {
        if (callback) try { callback(user, message); } catch (...) {}
    }
};

struct Host {
    rsf_sr_stage_resources* resources{};
    const TwinLog* twin_log{}; // borrowed only while the swap owner prepares the frame
    Host() = default;
    Host(const Host&) = delete;
    Host& operator=(const Host&) = delete;
    ~Host() { rsf_anomaly_sr_stage_drop_resources(resources); }
    const rsf_sr_pass* pass() const noexcept { return rsf_anomaly_sr_stage_pass(resources); }
    void* depth_texture() const noexcept { return rsf_anomaly_sr_stage_depth(resources); }
    bool read(const void* address, void* output, std::size_t size) noexcept { return guarded_copy(output, address, size); }
    bool write(void* address, const void* input, std::size_t size) noexcept { return guarded_copy(address, input, size); }
    ID3D11Device* device() noexcept { ID3D11Device* value{}; return read_global(srp().hw_device_rva, value) ? value : nullptr; }
    ID3D11DeviceContext* context() noexcept { ID3D11DeviceContext* value{}; return read_global(srp().hw_context_rva, value) ? value : nullptr; }
    MakeResult make_twins(const TwinRequest& request, TwinSet& out) noexcept {
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
    void drop_twins() noexcept {
        // Reference counts after our own views are gone. The engine's surface_get() before its generic_temp copy
        // takes references it never releases (stock leaks them on its own targets too), so this is diagnostic only.
        rsf_anomaly_sr_stage_retire_twins(resources, rsf_anomaly_sr_twin_refs);
    }
    void set_viewport(uint32_t width, uint32_t height) noexcept { set_full_viewport(context(), width, height); }
    bool upload_projection(const void* matrix) noexcept { return call_set_projection(matrix); }
    bool set_mip_bias(float value) noexcept { return call_set_mip_bias(value); }
    uint32_t correct_position_constant(void* constant, float x, float y) noexcept {
        return rsf_anomaly_sr_correct_position_constant(&position_layout, &position_constants,
            reinterpret_cast<uintptr_t>(constant), x, y);
    }
};

// ---- process state --------------------------------------------------------------------------------------------
// The leaf objects below are used under the Rust stage lock (the callbacks run inside its calls).
Host host;
const rsf_anomaly_swap_platform swap_platform{
    .user = &host,
    .read = [](void*, const void* from, void* to, size_t size) { return host.read(from, to, size); },
    .write = [](void*, void* to, const void* from, size_t size) { return host.write(to, from, size); },
    .make_twins = [](void*, const TwinRequest* request, TwinSet* twins) {
        return static_cast<uint32_t>(host.make_twins(*request, *twins));
    },
    .drop_twins = [](void*) { host.drop_twins(); },
    // The production seam passes a scoped operation directly to the Rust owner.
    .round_trip = nullptr,
    .upscale = nullptr,
    .set_viewport = [](void*, uint32_t width, uint32_t height) { host.set_viewport(width, height); },
    .upload_projection = [](void*, const void* matrix) { return host.upload_projection(matrix); },
    .set_mip_bias = [](void*, float value) { return host.set_mip_bias(value); },
    .correct_position_constant = [](void*, void* constant, float x, float y) {
        return host.correct_position_constant(constant, x, y);
    },
};
struct SwapOwner {
    rsf_anomaly_swap* state = rsf_anomaly_swap_create_scoped(&verified_layout, &swap_platform);
    ~SwapOwner() { rsf_anomaly_swap_destroy(state); }
};
SwapOwner swap;
constexpr rsf_anomaly_swap_view unavailable{.refusal = "The SR swap owner could not be created."};
rsf_anomaly_hook_set hooks{};

std::array<void*, 5> originals{};
using Unary = void(__fastcall*)(void*);

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
    const rsf_anomaly_guard_image checked{reinterpret_cast<const uint8_t*>(module), build->image_size(), guarded_copy};
    return rsf_anomaly_guard_site(&checked, site.rva, reinterpret_cast<const uint8_t*>(site.expected.data())) &&
        rsf_anomaly_slot_install(RSF_ANOMALY_POSITION_SLOT, &checked,
        srp().position_setup_vtable_rva, site.rva, 0, reinterpret_cast<void*>(&position_setup_hook), rsf_anomaly_compare_slot);
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
    if (rsf_anomaly_hooks_created(&hooks)) return rsf_anomaly_hooks_enable(&hooks);
    if (!build || !image) return false;
    const auto site = [](std::size_t index, void* detour, std::size_t original) {
        const auto& s = srp().sr_function_sites[index];
        rsf_anomaly_hook_site hook{s.name, s.rva, {}, detour, &originals[original]};
        std::memcpy(hook.expected, s.expected.data(), sizeof(hook.expected));
        return hook;
    };
    const std::array<rsf_anomaly_hook_site, 5> sites{{site(site_phase_scene_prepare, reinterpret_cast<void*>(&prepare_hook), 0),
        site(site_phase_combine, reinterpret_cast<void*>(&combine_hook), 1),
        site(site_phase_sunshafts, reinterpret_cast<void*>(&sunshafts_hook), 2),
        site(site_phase_blur, reinterpret_cast<void*>(&blur_hook), 3),
        site(site_sun_cascades, reinterpret_cast<void*>(&sun_cascades_hook), 4)}};
    return rsf_anomaly_hooks_install(&hooks, image, build->image_size(), sites.data(), sites.size());
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
        return rsf_anomaly_sr_view_resource(base);
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
    .close_stale = [] { rsf_anomaly_swap_close_stale(swap.state); },
    .prepare = [](const rsf_anomaly_sr_frame_facts* in, rsf_anomaly_sr_log log, void* user) {
        const TwinLog call_log{log, user};
        const auto* previous = host.twin_log;
        host.twin_log = &call_log;
        const bool prepared = rsf_anomaly_swap_prepare(swap.state, in);
        host.twin_log = previous;
        return prepared;
    },
    .prepared = [] { rsf_anomaly_swap_prepared(swap.state); },
    .swap_handle = []() -> void* { return swap.state; },
    .combine_end = [] { rsf_anomaly_swap_combine_end(swap.state); },
    .frame_end = [] { rsf_anomaly_swap_frame_end(swap.state); },
    .rm_near = [] { rsf_anomaly_swap_rm_near(swap.state); },
    .sun_begin = [] { return rsf_anomaly_swap_sun_begin(swap.state); },
    .sun_end = [](bool suspended) { rsf_anomaly_swap_sun_end(swap.state, suspended); },
    .position_setup = [](void* constant) { rsf_anomaly_swap_position_constant(swap.state, constant); },
    .shutdown = [] { return rsf_anomaly_swap_shutdown(swap.state); },
    .set_swap_enabled = [](bool enabled) { rsf_anomaly_swap_set_enabled(swap.state, enabled); },
    .controller = [](rsf_anomaly_sr_controller* out) {
        const auto& view = swap.state ? *rsf_anomaly_swap_view_of(swap.state) : unavailable;
        *out = {view.open, view.refusal != nullptr, view.jitter_applied, view.mip_bias_applied,
            view.stage, view.render_width, view.render_height, view.window_mip_bias,
            view.saved_mip_bias, view.counters.twin_generations, view.refusal};
    },
    .swap_counters = [](rsf_anomaly_sr_swap_counters* out) {
        *out = swap.state ? rsf_anomaly_swap_view_of(swap.state)->counters : rsf_anomaly_swap_counters{};
    },
    .timer_retain_device = rsf_anomaly_sr_timer_retain_device,
    .timer_create_query = rsf_anomaly_sr_timer_create_query,
    .timer_issue = rsf_anomaly_sr_timer_issue,
    .timer_clock = rsf_anomaly_sr_timer_clock,
    .timer_timestamp = rsf_anomaly_sr_timer_timestamp,
    .host_stats = [](uint64_t* bytes, uint64_t* refs) { rsf_anomaly_sr_stage_resource_stats(host.resources, bytes, refs); },
    .host_reset = [] { rsf_anomaly_sr_stage_reset_pass(host.resources); },
    .immediate_context = rsf_anomaly_sr_immediate_context,
    .context_device = rsf_anomaly_sr_context_device,
    .copy_subresource = rsf_anomaly_sr_copy_subresource,
    .release = rsf_anomaly_sr_release,
    .map_read = rsf_anomaly_sr_map_read,
    .unmap = rsf_anomaly_sr_unmap,
    .pixel_bytes = rsf_texture_pixel_bytes,
    .draw_pass = [](uint32_t kind, const rsf_sr_draw_views* views) {
        return rsf_anomaly_sr_run_pass(host.pass(), host.context(), kind, views) != 0;
    },
    .temporal_ready = [] { return rsf_anomaly_sr_temporal_ready(host.pass()) && host.depth_texture(); },
    .temporal_platform = []() -> const void* { return &d3d11_platform(); },
    .motion_target = [](rsf_anomaly_sr_motion_target* out) {
        const auto target = motion::frame_target();
        *out = {target.texture, target.written, target.main, target.frame, target.width, target.height};
        return target.texture != nullptr;
    },
    .create_motion_view = rsf_d3d11_create_shader_view,
    .depth_texture = [] { return host.depth_texture(); },
    // Registration connects these directly to the Rust temporal driver.
    .driver_bind = nullptr,
    .driver_enabled = nullptr,
    .driver_output_format = nullptr,
    .driver_planned = nullptr,
    .driver_boundary = nullptr,
    .driver_evaluate = nullptr,
    .driver_request_dump = nullptr,
    .driver_fallback = nullptr,
    .driver_count_prepared = nullptr,
    .driver_stop = nullptr,
    .driver_report = nullptr,
    .driver_overlay_report = nullptr,
    .driver_request = nullptr,
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
        const rsf_anomaly_guard_image checked{static_cast<const uint8_t*>(module), image_size, guarded_copy};
        return rsf_anomaly_sr_guard_failure(&checked, &guards, world_window, temporal);
    },
    .hooks_created = [] { return rsf_anomaly_hooks_created(&hooks); },
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
        (void)rsf_anomaly_hooks_disable(&hooks);
    },
    .set_rm_near = [](bool on) { geometry::geometry_set_rm_near_callback(on ? &rm_near_callback : nullptr); },
    .remove_hooks = [] {
        geometry::geometry_set_rm_near_callback(nullptr);
        const bool restored = rsf_anomaly_slot_set(RSF_ANOMALY_POSITION_SLOT, false);
        const bool disabled = rsf_anomaly_hooks_disable(&hooks);
        return restored && disabled;
    },
    .ini_int = rsf_anomaly_sr_ini_int,
    .ini_text = rsf_anomaly_sr_ini_text,
    .persist_int = rsf_anomaly_sr_persist_int,
    .persist_text = rsf_anomaly_sr_persist_text,
    .apply_colour_transport = rsf_anomaly_runtime_set_colour_transport,
    .configured = nullptr,
    .guarded_log = [](rsf_anomaly_sr_log log, void* user, const char* message) {
        if (log) try { log(user, message); } catch (...) {}
    },
};
void bind_platform() noexcept { rsf_anomaly_sr_stage_register(&platform); }
}

JitterState current_jitter() noexcept {
    rsf_anomaly_sr_jitter_state state{};
    rsf_anomaly_sr_stage_jitter(&state);
    return std::bit_cast<JitterState>(state);
}
JitterUv current_jitter_uv() noexcept {
    rsf_anomaly_sr_jitter_uv uv{};
    rsf_anomaly_sr_stage_jitter_uv(&uv);
    return {uv.current_u, uv.current_v, uv.previous_u, uv.previous_v};
}

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

// C entries for the bootstrap and observer leaves (stage_entries.h).
extern "C" bool rsf_anomaly_sr_configure_native(const wchar_t* ini, bool apply, rsf_game_log_fn log) noexcept {
    return rsf::anomaly::sr::configure(ini, apply, log, nullptr);
}
extern "C" bool rsf_anomaly_sr_install_native(void* image, size_t build) noexcept {
    if (build >= rsf::anomaly::build_profiles.size()) return false;
    const auto& selected = rsf::anomaly::build_profiles[build];
    return rsf::anomaly::sr::install(static_cast<std::byte*>(image), selected.image_size(), selected);
}
extern "C" uint32_t rsf_anomaly_sr_depth_rva(size_t build) noexcept {
    return build < rsf::anomaly::build_profiles.size() ? rsf::anomaly::build_profiles[build].sr->hw_base_zb_rva : 0;
}
