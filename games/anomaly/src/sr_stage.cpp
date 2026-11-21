// SPDX-License-Identifier: MIT
// Native leaves of the SR stage driver. sr_stage.rs owns the lifecycle, view admission, the H1 and seam ordering,
// jitter and camera publication, capture and burst bookkeeping, options and status (sr_stage_platform.h). This file
// keeps the guarded engine reads, the stage hooks and trampolines, the SwapController with its D3D11 host, the jitter
// and plan math objects and the TemporalDriver facade.
#include "sr_stage.h"
#include "sr_stage_platform.h"
#include "adapter_options.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include "native_observer.h"
#include "motion_stage.h"
#include "sr_d3d11.h"
#include "build_profile.h"
#include "sr_jitter.h"
#include "sr_plan.h"
#include "sr_swap.h"
#include "sr_reconstruction.h"
#include "sr_temporal.h"
#include <rescaleframe/game_frame.h>
#include <rescaleframe/overlay_api.h>
#include <rescaleframe/texture_dump.h>
#include <filesystem>
#include <windows.h>
#include <algorithm>
#include <cstdio>
#include <cmath>
#include <cstring>
#include <cwchar>
#include <string>
#include <string_view>

extern "C" {
void rsf_anomaly_sr_stage_register(const rsf_anomaly_sr_stage_platform*) noexcept;
void rsf_anomaly_sr_stage_read_options(const void*, rsf_anomaly_sr_stage_config*) noexcept;
float rsf_anomaly_sr_stage_read_scale(const void*, bool*) noexcept;
float rsf_anomaly_sr_stage_read_mip_bias(const void*, bool*) noexcept;
bool rsf_anomaly_sr_stage_configure(const rsf_anomaly_sr_stage_config*, rsf_anomaly_sr_log, void*) noexcept;
int32_t rsf_anomaly_sr_stage_option(uint32_t) noexcept;
void rsf_anomaly_sr_stage_set_option(uint32_t, int32_t) noexcept;
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
static_assert(static_cast<std::size_t>(Skip::Count) == RSF_ANOMALY_SR_SKIPS &&
    static_cast<std::size_t>(Restore::Count) == RSF_ANOMALY_SR_RESTORES);
static_assert(static_cast<uint8_t>(Refusal::Prepare) == 2 && static_cast<uint8_t>(Refusal::Reinsert) == 5);
static_assert(DXGI_FORMAT_R16G16B16A16_FLOAT == 10 && RSF_DUMP_VIEW_HDR == 3 && RSF_DUMP_VIEW_DEPTH == 4 &&
    RSF_DUMP_VIEW_DENSE_MOTION == 7 && RSF_PHASE_RENDER_SUBMIT == 2 && RSF_SCREEN_UNKNOWN == 0);
namespace {
// ---- configuration -------------------------------------------------------------------------------------------
// The native copy of the configured options: the INI and vendor paths, and the stage flags the other stages read.
StageOptions configured_options{};
rsf_game_log_fn log_fn{};
void* log_user{};
void say(const char* message) noexcept {
    if (log_fn) try { log_fn(log_user, message); } catch (...) {}
}

// ---- engine memory ------------------------------------------------------------------------------------------
bool guarded_copy(void* output, const void* input, std::size_t size) noexcept {
    if (!output || !input) return false;
    __try { std::memcpy(output, input, size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
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

// True while the engine draws its loading screen over the world: the precache frames and the "Press any key to continue"
// wait (sr_profile.h). Unreadable counts as loading, so a capture never starts on a guess.
bool loading_screen_active() noexcept {
    uint8_t registered{};
    uint32_t precache{};
    return !read_global(srp().load_screen_registered_rva, registered) || !read_global(srp().device_precache_frame_rva, precache) ||
        registered != 0 || precache != 0;
}
// An owned staging copy of a single-sample texture (the Stage 3 burst copies frames here and maps them once at the end).
void* staging_copy(void* context, void* source) noexcept {
    auto* ctx = static_cast<ID3D11DeviceContext*>(context);
    if (!ctx || !source) return nullptr;
    ComPtr<ID3D11Texture2D> texture;
    if (FAILED(static_cast<IUnknown*>(source)->QueryInterface(IID_PPV_ARGS(&texture)))) return nullptr;
    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    if (desc.SampleDesc.Count != 1) return nullptr;
    desc.MipLevels = desc.ArraySize = 1;
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    desc.MiscFlags = 0;
    ComPtr<ID3D11Device> device;
    ctx->GetDevice(&device);
    ComPtr<ID3D11Texture2D> copy;
    if (!device || FAILED(device->CreateTexture2D(&desc, nullptr, &copy))) return nullptr;
    ctx->CopySubresourceRegion(copy.Get(), 0, 0, 0, 0, texture.Get(), 0, nullptr);
    return copy.Detach();
}

// ---- the D3D11/engine host behind the swap controller --------------------------------------------------------
struct Host final : SwapHost {
    std::array<Twin, slot_count> twins{};
    DepthTwin depth{};
    ScratchTarget scratch{};
    RoundTripPass pass;
    UpscalePass upscale_pass;
    TemporalTargets temporal{};
    ComPtr<ID3D11ShaderResourceView> motion_view;   // SRV of the motion stage's target, rebuilt when it changes
    void* motion_view_texture{};
    uint64_t last_drop_refs{};
    uint64_t twin_bytes{};
    // While controller.seam() runs: the Rust scope lent by the seam call, and the seam being closed.
    void* cookie{};
    const UpscaleInputs* seam_inputs{};
    const UpscaleViews* seam_views{};
    const TargetFields* trip_twin{};
    const TargetFields* trip_engine{};
    bool read(const void* address, void* output, std::size_t size) noexcept override { return guarded_copy(output, address, size); }
    bool write(void* address, const void* input, std::size_t size) noexcept override { return guarded_copy(address, input, size); }
    ID3D11Device* device() noexcept { ID3D11Device* value{}; return read_global(srp().hw_device_rva, value) ? value : nullptr; }
    ID3D11DeviceContext* context() noexcept { ID3D11DeviceContext* value{}; return read_global(srp().hw_context_rva, value) ? value : nullptr; }
    static MakeResult result_of(TwinError error) noexcept {
        return error == TwinError::None ? MakeResult::Created : error == TwinError::Create ? MakeResult::Failed
            : error == TwinError::Depth ? MakeResult::Depth : MakeResult::Refused;
    }
    static uint64_t bytes_of(ID3D11Texture2D* texture) noexcept {
        if (!texture) return 0;
        D3D11_TEXTURE2D_DESC desc{};
        texture->GetDesc(&desc);
        const uint64_t texel = desc.Format == DXGI_FORMAT_R16G16B16A16_FLOAT ? 8 : desc.Format == DXGI_FORMAT_R16_FLOAT ? 2 : 4;
        return texel * desc.Width * desc.Height;
    }
    MakeResult make_twins(const TwinRequest& request, TwinSet& out) noexcept override {
        auto* d3d = device();
        if (!d3d) { say("Anomaly SR stage: HW.pDevice is null; twins not created."); return MakeResult::Failed; }
        if (request.stage == 1 && (!pass.ready() || pass.device() != d3d) && !pass.create(d3d, log_fn, log_user)) {
            say("Anomaly SR stage: round-trip shader could not be created.");
            return MakeResult::Refused;
        }
        if (request.stage == 2 && (!upscale_pass.ready() || upscale_pass.device() != d3d) && !upscale_pass.create(d3d, log_fn, log_user)) {
            say("Anomaly SR stage: upscale shader could not be created.");
            return MakeResult::Refused;
        }
        twin_bytes = 0;
        uint32_t made = 0;
        for (std::size_t i = 0; i < slot_count; ++i) {
            const auto& spec = request.spec[i];
            if (!spec.present) continue;
            const auto& engine = request.engine[i].fields;
            const auto error = request.stage == 1
                ? make_twin(d3d, static_cast<ID3D11Resource*>(engine.surface), engine.uav != nullptr, twins[i])
                : make_world_twin(d3d, engine, spec.width, spec.height, spec.promote, twins[i]);
            if (error != TwinError::None) {
                char text[200];
                std::snprintf(text, sizeof(text), "Anomaly SR stage: twin of %s refused (%s) at %ux%u; this frame stays stock.",
                    sr_world_targets[i].name, twin_error_name(error), spec.width, spec.height);
                say(text);
                twins = {};
                return result_of(error);
            }
            out.targets[i] = twins[i].fields;
            twin_bytes += bytes_of(twins[i].texture.Get());
            ++made;
        }
        if (request.stage == 2) {
            const auto error = make_depth_twin(d3d, static_cast<ID3D11DepthStencilView*>(request.engine_dsv),
                request.output_width, request.output_height, request.render_width, request.render_height, depth);
            if (error != TwinError::None) {
                char text[160];
                std::snprintf(text, sizeof(text), "Anomaly SR stage: depth twin refused (%s) at %ux%u.", twin_error_name(error),
                    request.render_width, request.render_height);
                say(text);
                twins = {};
                return result_of(error);
            }
            out.depth_dsv = depth.dsv.Get();
            out.depth_srv = depth.srv.Get();
            out.depth_width = depth.width;
            out.depth_height = depth.height;
            twin_bytes += 4ull * depth.width * depth.height;
            const auto scratch_error = make_scratch_target(d3d, static_cast<ID3D11RenderTargetView*>(request.engine_rtv),
                request.render_width, request.render_height, scratch);
            if (scratch_error != TwinError::None) {
                char text[160];
                std::snprintf(text, sizeof(text), "Anomaly SR stage: HW.pBaseRT stand-in refused (%s) at %ux%u.", twin_error_name(scratch_error),
                    request.render_width, request.render_height);
                say(text);
                twins = {};
                depth = {};
                return result_of(scratch_error);
            }
            out.base_rt = scratch.rtv.Get();
            twin_bytes += bytes_of(scratch.texture.Get());
        }
        char text[260];
        if (request.stage == 1)
            std::snprintf(text, sizeof(text), "Anomaly SR stage: FP16 twins created for generic0/generic_temp at %ux%u (uav=%d/%d).",
                out.targets[0].width, out.targets[0].height, out.targets[0].uav ? 1 : 0, out.targets[1].uav ? 1 : 0);
        else
            std::snprintf(text, sizeof(text),
                "Anomaly SR stage 2: %u world twins, a depth twin and a HW.pBaseRT stand-in created at render %ux%u for output %ux%u (%.1f MiB).",
                made, request.render_width, request.render_height, request.output_width, request.output_height,
                double(twin_bytes) / (1024.0 * 1024.0));
        say(text);
        return MakeResult::Created;
    }
    void drop_twins() noexcept override {
        // Reference counts after our own views are gone. The engine's surface_get() before its generic_temp copy
        // takes references it never releases (stock leaks them on its own targets too), so this is diagnostic only.
        uint64_t refs{};
        for (auto& twin : twins) {
            twin.rtv.Reset(); twin.srv.Reset(); twin.uav.Reset();
            if (twin.texture) { twin.texture->AddRef(); refs += twin.texture->Release() - 1; }
            twin = {};
        }
        depth = {};
        scratch = {};
        temporal = {};
        motion_view.Reset();
        motion_view_texture = nullptr;
        last_drop_refs = refs;
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
        seam_inputs = &in;
        seam_views = &views;
        const bool ok = rsf_anomaly_sr_stage_upscale(cookie, &seam);
        seam_inputs = nullptr;
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
        trip_twin = &twin;
        trip_engine = &engine;
        const bool ok = rsf_anomaly_sr_stage_round_trip(cookie, &seam);
        trip_twin = trip_engine = nullptr;
        return ok;
    }
};

// ---- process state --------------------------------------------------------------------------------------------
// The leaf objects below are used under the Rust stage lock (the callbacks run inside its calls).
Host host;
SwapController controller{host};
HookSet hooks;
TemporalDriver driver;

// GPU time of the world window (H1 prologue to the S-B seam) and of the seam pass, from D3D11 timestamp queries read
// back without stalling a few frames later. Runs whenever the hooks are installed, so a capture-only stock run
// (SRStage=0, SRStageCapture=1) measures the same window at output size for the comparison.
struct GpuTimer {
    struct Set {
        ComPtr<ID3D11Query> disjoint, begin, seam, end;
        bool pending{}, swapped{};
    };
    std::array<Set, 4> sets{};
    ComPtr<ID3D11Device> device;
    std::size_t cursor{};
    bool open{}, seam_marked{};
    double world_ms[2]{}, seam_ms{};
    uint64_t samples[2]{}, seam_samples{}, skipped{};
    bool ready(ID3D11Device* d3d) noexcept {
        if (device.Get() == d3d && sets[0].end) return true;
        *this = {};
        if (!d3d) return false;
        D3D11_QUERY_DESC timestamp{D3D11_QUERY_TIMESTAMP, 0}, disjoint{D3D11_QUERY_TIMESTAMP_DISJOINT, 0};
        for (auto& set : sets)
            if (FAILED(d3d->CreateQuery(&disjoint, &set.disjoint)) || FAILED(d3d->CreateQuery(&timestamp, &set.begin)) ||
                FAILED(d3d->CreateQuery(&timestamp, &set.seam)) || FAILED(d3d->CreateQuery(&timestamp, &set.end))) {
                *this = {};
                return false;
            }
        device = d3d;
        return true;
    }
    void collect(ID3D11DeviceContext* ctx) noexcept {
        for (auto& set : sets) {
            if (!set.pending) continue;
            D3D11_QUERY_DATA_TIMESTAMP_DISJOINT clock{};
            uint64_t begin{}, seam{}, end{};
            if (ctx->GetData(set.disjoint.Get(), &clock, sizeof(clock), D3D11_ASYNC_GETDATA_DONOTFLUSH) != S_OK ||
                ctx->GetData(set.begin.Get(), &begin, sizeof(begin), D3D11_ASYNC_GETDATA_DONOTFLUSH) != S_OK ||
                ctx->GetData(set.seam.Get(), &seam, sizeof(seam), D3D11_ASYNC_GETDATA_DONOTFLUSH) != S_OK ||
                ctx->GetData(set.end.Get(), &end, sizeof(end), D3D11_ASYNC_GETDATA_DONOTFLUSH) != S_OK)
                continue;
            set.pending = false;
            if (clock.Disjoint || !clock.Frequency || seam < begin || end < seam) { ++skipped; continue; }
            const double scale = 1000.0 / double(clock.Frequency);
            world_ms[set.swapped ? 1 : 0] += double(seam - begin) * scale;
            ++samples[set.swapped ? 1 : 0];
            if (set.swapped) { seam_ms += double(end - seam) * scale; ++seam_samples; }
        }
    }
    // H1 prologue.
    void begin(ID3D11Device* d3d, ID3D11DeviceContext* ctx) noexcept {
        if (!ctx || !ready(d3d)) return;
        collect(ctx);
        if (open) { ctx->End(sets[cursor].disjoint.Get()); open = false; }   // an unfinished set (no seam): dropped
        auto& set = sets[cursor];
        if (set.pending) { ++skipped; return; }   // the GPU is more than four frames behind: skip this frame
        ctx->Begin(set.disjoint.Get());
        ctx->End(set.begin.Get());
        open = true;
        seam_marked = false;
    }
    // First seam call of the frame, before the restore (end of the world window).
    void seam(ID3D11DeviceContext* ctx, bool swapped) noexcept {
        if (!open || seam_marked || !ctx) return;
        ctx->End(sets[cursor].seam.Get());
        sets[cursor].swapped = swapped;
        seam_marked = true;
    }
    // After the seam pass (or immediately on a stock frame).
    void finish(ID3D11DeviceContext* ctx) noexcept {
        if (!open || !seam_marked || !ctx) return;
        auto& set = sets[cursor];
        ctx->End(set.end.Get());
        ctx->End(set.disjoint.Get());
        set.pending = true;
        open = false;
        cursor = (cursor + 1) % sets.size();
    }
};
GpuTimer gpu_timer;
std::array<void*, 6> originals{};
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
rsf_anomaly_sr_settings to_c(const SrSettings& s) noexcept { return {s.enabled, s.backend, s.quality, s.preset}; }
std::wstring widen(const char* text) {
    std::wstring out;
    for (; text && *text; ++text) out.push_back(static_cast<unsigned char>(*text));
    return out;
}

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
    reinterpret_cast<void(__fastcall*)(void*, void*)>(originals[5])(self, constant);
    rsf_anomaly_sr_stage_position_setup(constant);
}
// cl_pos_decompress_params::setup is only ever reached through the binder's vtable (R_constant_setup::setup, slot 0).
// The AVX build's body has a VEX instruction (vmovss) seven bytes in, which MinHook's decoder cannot relocate
// (MH_ERROR_UNSUPPORTED_FUNCTION on the real executable; the SSE build is fine). So this site swaps the vtable slot on
// both builds instead of detouring the body. The slot must hold the profiled function, and the function its guard.
void** position_slot{};
bool set_position_slot(void* value) noexcept {
    DWORD old{};
    if (!position_slot || !VirtualProtect(position_slot, sizeof(void*), PAGE_READWRITE, &old)) return false;
    *position_slot = value;
    DWORD ignored{};
    VirtualProtect(position_slot, sizeof(void*), old, &ignored);
    return true;
}
bool install_position_slot(std::byte* module) noexcept {
    const auto& site = srp().sr_function_sites[site_position_setup];
    auto* function = module + site.rva;
    auto* slot = reinterpret_cast<void**>(module + srp().position_setup_vtable_rva);
    if (position_slot == slot && *slot == reinterpret_cast<void*>(&position_setup_hook)) return true;   // already swapped
    if (*slot != function || std::memcmp(function, site.expected.data(), site.expected.size()) != 0) return false;
    originals[5] = function;
    position_slot = slot;
    return set_position_slot(reinterpret_cast<void*>(&position_setup_hook));
}
void remove_position_slot() noexcept {
    if (position_slot && *position_slot == reinterpret_cast<void*>(&position_setup_hook)) (void)set_position_slot(originals[5]);
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

bool bytes_match(const std::byte* address, const char* hex) noexcept {
    const auto digit = [](char c) { return c <= '9' ? c - '0' : c - 'a' + 10; };
    for (std::size_t i = 0; hex[2 * i] && hex[2 * i + 1]; ++i) {
        std::byte value{};
        if (!guarded_copy(&value, address + i, 1) || value != static_cast<std::byte>(digit(hex[2 * i]) * 16 + digit(hex[2 * i + 1])))
            return false;
    }
    return true;
}
// Stage 3 calls R_xforms::set_P and SetMipLODBias, and relies on every first-person projection site uploading with
// set_P and then calling rmNear (the shared slot). Each must still be the verified code.
bool stage3_guards_match(std::byte* module, uint32_t image_size, const SrProfile& sr_profile, const char** failed) noexcept {
    const auto& set_p = sr_profile.sr_function_sites[site_set_p];
    const auto& set_bias = sr_profile.sr_function_sites[site_set_mip_lod_bias];
    const auto& rm_near = sr_profile.sr_function_sites[site_rm_near];
    for (const auto& guard_site : sr_profile.sr_unique_guards)
        if (guard_site.rva == set_p.rva && !bytes_match(module + guard_site.rva, guard_site.expected_hex)) { *failed = set_p.name; return false; }
    std::array<std::byte, 16> bytes{};
    if (set_bias.rva > image_size - bytes.size() || !guarded_copy(bytes.data(), module + set_bias.rva, bytes.size()) ||
        bytes != set_bias.expected) { *failed = set_bias.name; return false; }
    const auto rel_call_to = [&](uint32_t at, uint32_t target) {
        std::array<std::byte, 5> call{};
        if (at > image_size - call.size() || !guarded_copy(call.data(), module + at, call.size()) || call[0] != std::byte{0xe8}) return false;
        int32_t displacement{};
        std::memcpy(&displacement, call.data() + 1, sizeof(displacement));
        return static_cast<int64_t>(at) + 5 + displacement == static_cast<int64_t>(target);
    };
    for (const auto& site : sr_profile.sr_first_person_sites) {
        // `call R_xforms::set_P`, then within 0x120 bytes `call [rax+0x1b8]` (vtable slot 55) or `call CRender::rmNear`.
        if (!rel_call_to(site.set_p_call, set_p.rva) || site.rm_near_call <= site.set_p_call ||
            site.rm_near_call - site.set_p_call >= 0x120 ||
            !(bytes_match(module + site.rm_near_call, "ff90b8010000") || rel_call_to(site.rm_near_call, rm_near.rva))) {
            *failed = site.name; return false;
        }
    }
    return true;
}
bool guards_match(std::byte* module, uint32_t image_size, const BuildProfile& selected, bool world_window, bool temporal,
    const char** failed) noexcept {
    const auto& sr_profile = *selected.sr;
    // Stage 2 writes the SSAO helper's static viewport: its code, which encodes both data addresses, must match.
    if (world_window && !bytes_match(module + sr_profile.ssao_set_viewport_rva, sr_profile.ssao_set_viewport_code)) {
        *failed = "set_viewport (r4_rendertarget_phase_ssao.cpp:3)"; return false;
    }
    if (temporal && !stage3_guards_match(module, image_size, sr_profile, failed)) return false;
    if (temporal) {
        for (const auto& site : sr_profile.reconstruction_guards) {
            const auto length = std::strlen(site.expected_hex) / 2;
            if (site.rva > image_size || length > image_size - site.rva || !bytes_match(module + site.rva, site.expected_hex)) {
                *failed = site.name; return false;
            }
        }
    }
    // rmNear is not checked here: the geometry producer verified and patched it, and Stage 1 reaches it only through
    // the shared slot.
    for (const auto index : {site_phase_scene_prepare, site_phase_combine, site_phase_sunshafts, site_phase_blur, site_sun_cascades, site_position_setup}) {
        const auto& site = sr_profile.sr_function_sites[index];
        std::array<std::byte, 16> bytes{};
        if (site.rva > image_size - bytes.size() || !guarded_copy(bytes.data(), module + site.rva, bytes.size()) || bytes != site.expected) {
            *failed = site.name; return false;
        }
    }
    for (const auto& guard_site : sr_profile.sr_unique_guards) {
        if (guard_site.rva != sr_profile.sr_function_sites[site_phase_blur].rva &&
            guard_site.rva != sr_profile.sr_function_sites[site_sun_cascades].rva) continue;
        if (!bytes_match(module + guard_site.rva, guard_site.expected_hex)) { *failed = guard_site.name; return false; }
    }
    for (const auto& site : sr_profile.sr_call_sites) {
        std::array<std::byte, 16> bytes{};
        if (!guarded_copy(bytes.data(), module + site.rva, bytes.size()) || bytes != site.expected) { *failed = site.name; return false; }
    }
    return true;
}
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

std::string utf8(const std::wstring& value) {
    if (value.empty()) return {};
    const int size = WideCharToMultiByte(CP_UTF8, 0, value.c_str(), int(value.size()), nullptr, 0, nullptr, nullptr);
    std::string result(size > 0 ? size : 0, '\0');
    if (size > 0) WideCharToMultiByte(CP_UTF8, 0, value.c_str(), int(value.size()), result.data(), size, nullptr, nullptr);
    return result;
}
// Raw rows (tightly packed) of a mapped staging texture.
bool write_raw(const wchar_t* path, ID3D11DeviceContext* ctx, ID3D11Texture2D* texture, uint32_t& width, uint32_t& height,
    uint32_t& format) noexcept {
    if (!texture || !ctx || !path) return false;
    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    width = desc.Width;
    height = desc.Height;
    format = desc.Format;
    const uint32_t texel = desc.Format == DXGI_FORMAT_R16G16B16A16_FLOAT ? 8 : 4;
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(ctx->Map(texture, 0, D3D11_MAP_READ, 0, &mapped))) return false;
    FILE* file{};
    bool ok = _wfopen_s(&file, path, L"wb") == 0 && file;
    for (uint32_t row = 0; ok && row < desc.Height; ++row)
        ok = std::fwrite(static_cast<const std::byte*>(mapped.pData) + std::size_t(row) * mapped.RowPitch, texel, desc.Width, file) == desc.Width;
    if (file) std::fclose(file);
    ctx->Unmap(texture, 0);
    return ok;
}
bool file_exists(const std::wstring& directory, const wchar_t* name) noexcept try {
    const auto path = std::filesystem::path(directory) / name;
    const DWORD attributes = GetFileAttributesW(path.c_str());
    return attributes != INVALID_FILE_ATTRIBUTES && !(attributes & FILE_ATTRIBUTE_DIRECTORY);
} catch (...) { return false; }
void apply_colour_transport(bool transport) noexcept {
    using Set = void (*)(uint32_t);
    const HMODULE runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    if (const auto set = runtime ? reinterpret_cast<Set>(reinterpret_cast<void*>(
            GetProcAddress(runtime, "rsf_dlss_pipeline_set_colour_transport"))) : nullptr)
        set(transport ? 1u : 0u);
}

// ---- the leaf table ---------------------------------------------------------------------------------------------
const rsf_anomaly_sr_stage_platform platform{
    .frame_facts = [](void* self, bool world_window, bool temporal, rsf_anomaly_sr_frame_facts* out) {
        *out = {};
        std::byte* target{};
        if (read_global(srp().render_implementation_rva + render_target_offset, target) && target == self)
            out->render_target = target;   // phase_scene_prepare's `this` must be CRender::Target
        if (!image || !build) { out->scope = out->msaa = true; return; }
        out->backend = image + srp().backend_rva;
        read_global(srp().hw_device_rva, out->device);
        NativeObservation observation{sizeof(observation), observation_abi};
        if (rsf_anomaly_observation(&observation) == RSF_OK) out->generation = observation.resets;
        read_global(np().device_width_rva, out->device_width);
        read_global(np().device_height_rva, out->device_height);
        const auto view = geometry::current_render_view(image, *build);
        out->scope = !view || *view == temporal::ViewId::Scope;   // unreadable counts as not eligible
        uint32_t options3{};
        out->msaa = !read_global(srp().render_options_word3_rva, options3) || (options3 & dx10_msaa_bit);
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
    .loading_screen = [] { return loading_screen_active(); },
    .engine_frame = [] { uint32_t frame{}; read_global(np().device_frame_rva, frame); return frame; },
    .device_size = [](uint32_t* width, uint32_t* height) {
        read_global(np().device_width_rva, *width);
        read_global(np().device_height_rva, *height);
    },
    .read_camera = [](rsf_anomaly_sr_camera* out) {
        return read_global(srp().device_view_rva, out->view) && read_global(srp().device_project_rva, out->projection);
    },
    .seam_surface = [](void* self) -> void* {
        std::byte* target{};
        std::byte* crt{};
        void* surface{};
        if (read_global(srp().render_implementation_rva + render_target_offset, target) && target == self &&
            guarded_copy(&crt, target + 0x3a8, sizeof(crt)) && crt && guarded_copy(&surface, crt + crt_surface_offset, sizeof(surface)))
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
    .prepare = [](const rsf_anomaly_sr_frame_facts* in) {
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
        return controller.prepare(facts);
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
    .timer_begin = [] { gpu_timer.begin(host.device(), host.context()); },
    .timer_seam = [](bool swapped) { gpu_timer.seam(host.context(), swapped); },
    .timer_finish = [] { gpu_timer.finish(host.context()); },
    .timer_reset = [] { gpu_timer = {}; },
    .timer_sample = [](rsf_anomaly_sr_timer* out) {
        if (auto* ctx = host.context(); ctx && gpu_timer.device) gpu_timer.collect(ctx);
        *out = {{gpu_timer.world_ms[0], gpu_timer.world_ms[1]}, gpu_timer.seam_ms, {gpu_timer.samples[0], gpu_timer.samples[1]},
            gpu_timer.seam_samples, gpu_timer.skipped, gpu_timer.device != nullptr};
    },
    .host_stats = [](uint64_t* bytes, uint64_t* refs) { *bytes = host.twin_bytes; *refs = host.last_drop_refs; },
    .host_reset = [] {
        host.temporal = {};
        host.motion_view.Reset();
        host.motion_view_texture = nullptr;
        host.pass.reset();
        host.upscale_pass.reset();
    },
    .staging_copy = [](void* context, void* source) { return staging_copy(context, source); },
    .release = [](void* object) { if (object) static_cast<IUnknown*>(object)->Release(); },
    .write_raw = [](const uint16_t* path, void* context, void* texture, uint32_t* width, uint32_t* height, uint32_t* format) {
        return write_raw(reinterpret_cast<const wchar_t*>(path), static_cast<ID3D11DeviceContext*>(context),
            static_cast<ID3D11Texture2D*>(texture), *width, *height, *format);
    },
    .bilinear = [] { return host.seam_views && host.upscale_pass.run(host.context(), *host.seam_views); },
    .round_trip = [] {
        if (!host.trip_twin || !host.trip_engine) return false;
        const auto& twin = *host.trip_twin;
        const auto& engine = *host.trip_engine;
        return host.pass.run(host.context(), static_cast<ID3D11ShaderResourceView*>(twin.srv), twin.width, twin.height,
            static_cast<ID3D11RenderTargetView*>(engine.rtv), engine.width, engine.height);
    },
    .temporal_ready = [] { return host.seam_views && host.upscale_pass.temporal_ready() && host.depth.texture; },
    .temporal_targets = [](uint32_t format) -> int32_t {
        if (!host.seam_inputs) return -1;
        const auto& in = *host.seam_inputs;
        auto* d3d = host.device();
        const auto dxgi = static_cast<DXGI_FORMAT>(format);
        if (host.temporal.matches(d3d, in.render_width, in.render_height, in.output_width, in.output_height, dxgi)) return 0;
        return make_temporal_targets(d3d, in.render_width, in.render_height, in.output_width, in.output_height, dxgi, host.temporal) ==
            TwinError::None ? 1 : -1;
    },
    .motion_target = [](rsf_anomaly_sr_motion_target* out) {
        const auto target = motion::frame_target();
        *out = {target.texture, target.written, target.main, target.frame, target.width, target.height};
        return target.texture != nullptr;
    },
    .motion_view = [](void* texture) {
        if (host.motion_view_texture != texture) {
            host.motion_view.Reset();
            host.motion_view_texture = texture;
            auto* d3d = host.device();
            if (!d3d || FAILED(d3d->CreateShaderResourceView(static_cast<ID3D11Texture2D*>(texture), nullptr, &host.motion_view)))
                host.motion_view.Reset();
        }
        return host.motion_view != nullptr;
    },
    .seam_prepare = [](bool motion_present, bool first_person_zero) {
        if (!host.seam_views || !host.seam_inputs) return false;
        const auto& views = *host.seam_views;
        const auto& in = *host.seam_inputs;
        auto& t = host.temporal;
        const PrepareViews prepare{views.colour, motion_present ? host.motion_view.Get() : t.empty_motion_srv.Get(), views.depth,
            in.render_width, in.render_height, t.colour_rtv.Get(), t.motion_rtv.Get(), first_person_depth_limit,
            first_person_zero};
        return host.upscale_pass.prepare(host.context(), prepare);
    },
    .temporal_textures = [](rsf_anomaly_sr_temporal_textures* out) {
        *out = {host.temporal.colour.Get(), host.depth.texture.Get(), host.temporal.motion.Get(), host.temporal.output.Get()};
    },
    .reinsert = [] {
        return host.seam_views && host.upscale_pass.reinsert(host.context(), *host.seam_views, host.temporal.output_srv.Get());
    },
    .driver_bind = [] { return driver.bind(GetModuleHandleW(L"ReScaleFrame.Runtime.dll")); },
    .driver_enabled = [] { return driver.enabled(); },
    .driver_output_format = [] { return driver.output_format(); },
    .driver_planned = [](uint32_t ow, uint32_t oh, uint32_t* rw, uint32_t* rh) { return driver.planned_render(ow, oh, *rw, *rh); },
    .driver_boundary = [](uint32_t width, uint32_t height) { driver.boundary(host.device(), width, height); },
    .driver_evaluate = [](const rsf_anomaly_sr_pass* in, rsf_camera_frame* camera, uint32_t* backend) {
        auto& t = host.temporal;
        rsf_anomaly_sr_pass_inputs pass{};
        pass.frame = *in;
        pass.color = t.colour.Get();
        pass.color_output = t.output.Get();
        pass.depth = host.depth.texture.Get();
        pass.motion = t.motion.Get();
        pass.exposure = in->exposure_auto ? nullptr : t.exposure.Get();
        return driver.evaluate(host.context(), pass, {.sequence = in->reset_sequence, .cut = in->cut}, camera, backend);
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
        const char* failed = "unknown";
        return guards_match(static_cast<std::byte*>(module), image_size, *static_cast<const BuildProfile*>(selected), world_window,
            temporal, &failed) ? nullptr : failed;
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
    .disable_hooks = [] { (void)hooks.disable(); },
    .set_rm_near = [](bool on) { geometry::geometry_set_rm_near_callback(on ? &rm_near_callback : nullptr); },
    .remove_hooks = [] {
        geometry::geometry_set_rm_near_callback(nullptr);
        remove_position_slot();
        return hooks.disable();
    },
    .ini_int = [](const void* ini, const char* key, int32_t fallback) {
        try {
            return static_cast<int32_t>(GetPrivateProfileIntW(L"Anomaly", widen(key).c_str(), fallback, static_cast<const wchar_t*>(ini)));
        } catch (...) { return fallback; }
    },
    .ini_text = [](const void* ini, const char* key, const char* fallback, uint16_t* out, uint32_t capacity) -> uint32_t {
        try {
            return GetPrivateProfileStringW(L"Anomaly", widen(key).c_str(), widen(fallback).c_str(), reinterpret_cast<wchar_t*>(out),
                capacity, static_cast<const wchar_t*>(ini));
        } catch (...) { return 0; }
    },
    .configured_ini = []() -> const void* { return configured_options.ini.empty() ? nullptr : configured_options.ini.c_str(); },
    .persist_int = [](const char* key, int32_t value) { try { options::write_int(widen(key).c_str(), value); } catch (...) {} },
    .persist_text = [](const char* key, const char* value) {
        try { options::write_text(widen(key).c_str(), widen(value).c_str()); } catch (...) {}
    },
    .apply_colour_transport = [](bool transport) { apply_colour_transport(transport); },
    .vendor_file = [](uint32_t directory, const char* name) {
        try {
            const auto& o = configured_options;
            const std::wstring& root = directory == 0 ? o.streamline_dir : directory == 1 ? o.fidelityfx_dir : o.xess_dir;
            return file_exists(root, widen(name).c_str());
        } catch (...) { return false; }
    },
};
void bind_platform() noexcept { rsf_anomaly_sr_stage_register(&platform); }

rsf_anomaly_sr_stage_config to_c(const StageOptions& o) noexcept {
    return {o.stage, o.scale, o.scale_defaulted, o.jitter, o.mip_bias, o.mip_bias_offset, o.capture, o.capture_start,
        o.capture_count, o.capture_interval, o.exposure_auto, o.colour_transport, o.first_person_zero, o.require_signature,
        o.cut.distance, o.cut.degrees, to_c(o.settings)};
}

// The options the panel changes while the game runs. Render thread only (adapter_options.h). Rust applies and saves
// them; the native copy follows so options() reports the live values.
void refresh_live() noexcept {
    configured_options.colour_transport = rsf_anomaly_sr_stage_option(RSF_ANOMALY_SR_OPTION_COLOUR_TRANSPORT) == 0;
    configured_options.exposure_auto = rsf_anomaly_sr_stage_option(RSF_ANOMALY_SR_OPTION_EXPOSURE_AUTO) != 0;
    configured_options.first_person_zero = rsf_anomaly_sr_stage_option(RSF_ANOMALY_SR_OPTION_FIRST_PERSON_ZERO) != 0;
    configured_options.jitter = rsf_anomaly_sr_stage_option(RSF_ANOMALY_SR_OPTION_JITTER) != 0;
    configured_options.mip_bias = rsf_anomaly_sr_stage_option(RSF_ANOMALY_SR_OPTION_MIP_BIAS) != 0;
    configured_options.mip_bias_offset = float(rsf_anomaly_sr_stage_option(RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET)) / 10.0f;
}
template<uint32_t Id> int32_t live_get() noexcept { return rsf_anomaly_sr_stage_option(Id); }
template<uint32_t Id> void live_set(int32_t value) noexcept { rsf_anomaly_sr_stage_set_option(Id, value); refresh_live(); }
void register_live_options() noexcept {
    using options::add;
    add({"DLSS input: linear HDR colour", true, 0, 1, 1, 1, false,
        &live_get<RSF_ANOMALY_SR_OPTION_COLOUR_TRANSPORT>, &live_set<RSF_ANOMALY_SR_OPTION_COLOUR_TRANSPORT>});
    add({"DLSS auto exposure", true, 0, 1, 1, 1, false,
        &live_get<RSF_ANOMALY_SR_OPTION_EXPOSURE_AUTO>, &live_set<RSF_ANOMALY_SR_OPTION_EXPOSURE_AUTO>});
    add({"First-person pixels: zero motion", true, 0, 1, 1, 1, false,
        &live_get<RSF_ANOMALY_SR_OPTION_FIRST_PERSON_ZERO>, &live_set<RSF_ANOMALY_SR_OPTION_FIRST_PERSON_ZERO>});
    add({"Subpixel jitter (SR needs it; off = no upscaling)", true, 0, 1, 1, 1, false,
        &live_get<RSF_ANOMALY_SR_OPTION_JITTER>, &live_set<RSF_ANOMALY_SR_OPTION_JITTER>});
    add({"Texture mip bias", true, 0, 1, 1, 1, false,
        &live_get<RSF_ANOMALY_SR_OPTION_MIP_BIAS>, &live_set<RSF_ANOMALY_SR_OPTION_MIP_BIAS>});
    add({"Mip bias offset", false, -20, 0, 1, 10, false,
        &live_get<RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET>, &live_set<RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET>});
}
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

StageOptions read_options(const wchar_t* ini) noexcept try {
    StageOptions result{};
    if (!ini) return result;
    bind_platform();
    rsf_anomaly_sr_stage_config c{};
    rsf_anomaly_sr_stage_read_options(ini, &c);
    result.stage = c.stage;
    result.scale = c.scale;
    result.scale_defaulted = c.scale_defaulted;
    result.jitter = c.jitter;
    result.mip_bias = c.mip_bias;
    result.mip_bias_offset = c.mip_bias_offset;
    result.capture = c.capture;
    result.capture_start = c.capture_start;
    result.capture_count = c.capture_count;
    result.capture_interval = c.capture_interval;
    result.exposure_auto = c.exposure_auto;
    result.colour_transport = c.colour_transport;
    result.first_person_zero = c.first_person_zero;
    result.require_signature = c.require_signature;
    result.cut.distance = c.cut_distance;
    result.cut.degrees = c.cut_degrees;
    result.settings = to_native(c.settings);
    result.ini = ini;
    // Vendor runtimes: the package layout beside the INI (ReScaleFrame\streamline, fidelityfx, xess) unless overridden.
    const auto base = std::filesystem::path(ini).parent_path() / L"ReScaleFrame";
    const auto read_dir = [&](const wchar_t* key, const wchar_t* fallback) {
        wchar_t value[MAX_PATH]{};
        GetPrivateProfileStringW(L"Anomaly", key, L"", value, MAX_PATH, ini);
        return plain_directory(value[0] ? std::wstring(value) : (base / fallback).wstring());
    };
    result.streamline_dir = read_dir(L"SRStreamlineDir", L"streamline");
    result.fidelityfx_dir = read_dir(L"SRFidelityFXDir", L"fidelityfx");
    result.xess_dir = read_dir(L"SRXeSSDir", L"xess");
    return result;
} catch (...) { return {}; }
void set_scale(float value) noexcept { rsf_anomaly_sr_stage_set_scale(value); }

void configure(const StageOptions& value, rsf_game_log_fn log, void* user) noexcept {
    bind_platform();
    const auto config = to_c(value);
    if (!rsf_anomaly_sr_stage_configure(&config, log, user)) return;   // the hooks are installed
    try { configured_options = value; } catch (...) { configured_options = {}; }
    register_live_options();
    log_fn = log;
    log_user = user;
    if (value.sr_inputs()) {
        TemporalOptions temporal{};
        temporal.stage = value.stage;
        temporal.exposure_auto = value.exposure_auto;
        temporal.colour_transport = value.colour_transport;
        temporal.first_person_zero = value.first_person_zero;
        try {
            temporal.streamline = utf8(value.streamline_dir);
            temporal.fidelityfx = utf8(value.fidelityfx_dir);
            temporal.xess = utf8(value.xess_dir);
        } catch (...) {}
        temporal.require_signature = value.require_signature;
        temporal.initial = value.settings;
        driver.configure(temporal, log, user);
    }
}
const StageOptions& options() noexcept { return configured_options; }
bool active() noexcept { return rsf_anomaly_sr_stage_active(); }

bool install(std::byte* module, uint32_t image_size, const BuildProfile& selected) noexcept {
    bind_platform();
    return rsf_anomaly_sr_stage_install(module, image_size, selected.image_size(), &selected);
}
bool quiesce() noexcept { return rsf_anomaly_sr_stage_quiesce(); }
void frame_end() noexcept { rsf_anomaly_sr_stage_frame_end(); }
void report(void (*emit)(void*, const char*), void* user) noexcept { rsf_anomaly_sr_stage_report(emit, user); }

}
