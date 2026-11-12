// SPDX-License-Identifier: GPL-3.0-only
#include "sr_stage.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include "native_observer.h"
#include "motion_stage.h"
#include "sr_d3d11.h"
#include "sr_profile.h"
#include "sr_swap.h"
#include "sr_temporal.h"
#include <rescaleframe/game_frame.h>
#include <rescaleframe/overlay_api.h>
#include <rescaleframe/texture_dump.h>
#include <filesystem>
#include <windows.h>
#include <atomic>
#include <cstdio>
#include <cmath>
#include <cstring>
#include <cwchar>
#include <mutex>
#include <string>
#include <string_view>
#include <vector>

namespace rsf::anomaly::sr {
namespace {
// ---- configuration -------------------------------------------------------------------------------------------
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
template<class T> bool read_global(uint32_t rva, T& value) noexcept { return image && guarded_copy(&value, image + rva, sizeof(T)); }

// ---- core frame dump service (Runtime exports) ----------------------------------------------------------------
struct DumpApi {
    void (*configure)(const char*, const char*, const char*, rsf_dump_log_fn, void*){};
    void (*request)(){};
    void (*poll)(){};
    int (*requested)(){};
    int (*begin_record)(const rsf_frame_record*, uint32_t){};
    int (*texture_record)(void*, const rsf_frame_record*, const char*, const char*, void*, rsf_dump_view, float){};
    void (*complete)(const rsf_frame_record*, int32_t){};
    void (*cancel)(uint64_t){};
    bool bind(HMODULE runtime) noexcept {
        const auto get = [runtime](const char* name) { return reinterpret_cast<void*>(GetProcAddress(runtime, name)); };
        configure = reinterpret_cast<decltype(configure)>(get("rsf_input_dump_configure"));
        request = reinterpret_cast<decltype(request)>(get("rsf_input_dump_request"));
        poll = reinterpret_cast<decltype(poll)>(get("rsf_input_dump_poll"));
        requested = reinterpret_cast<decltype(requested)>(get("rsf_input_dump_requested"));
        begin_record = reinterpret_cast<decltype(begin_record)>(get("rsf_input_dump_begin_record"));
        texture_record = reinterpret_cast<decltype(texture_record)>(get("rsf_input_dump_texture_record"));
        complete = reinterpret_cast<decltype(complete)>(get("rsf_input_dump_complete"));
        cancel = reinterpret_cast<decltype(cancel)>(get("rsf_input_dump_cancel"));
        return configure && request && poll && requested && begin_record && texture_record && complete && cancel;
    }
};
DumpApi dump{};
bool dump_ready{};

struct Capture {
    bool active{};
    rsf_frame_record record{};
    uint32_t remaining{};
    uint64_t next{};
    uint64_t taken{}, images{}, failed{};
    bool seam_taken{};
};

// ---- Stage 3 engine calls (byte-guarded at install) --------------------------------------------------------------
// R_xforms::set_P(this = &RCache.xforms, rdx = const Fmatrix*) and dx10SamplerStateCache::SetMipLODBias(this =
// &SSManager, xmm1 = float), both x64 __fastcall (sr_profile.h sr_function_sites).
using SetProjection = void(__fastcall*)(void* xforms, const void* matrix);
using SetMipBias = void(__fastcall*)(void* cache, float bias);
SetProjection set_projection_fn{};
SetMipBias set_mip_bias_fn{};
bool call_set_projection(const void* matrix) noexcept {
    if (!set_projection_fn || !image) return false;
    __try { set_projection_fn(image + backend_rva + backend_xforms_offset, matrix); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
bool call_set_mip_bias(float value) noexcept {
    if (!set_mip_bias_fn || !image) return false;
    __try { set_mip_bias_fn(image + sampler_cache_rva, value); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

// True while the engine draws its loading screen over the world: the precache frames and the "Press any key to continue"
// wait (sr_profile.h). Unreadable counts as loading, so a capture never starts on a guess.
bool loading_screen_active() noexcept {
    uint8_t registered{};
    uint32_t precache{};
    return !read_global(load_screen_registered_rva, registered) || !read_global(device_precache_frame_rva, precache) ||
        registered != 0 || precache != 0;
}
// ---- Stage 3 jitter burst: consecutive render-size world frames of one view -------------------------------------
// The core frame dump service bundles one frame per request, so consecutive frames are copied into adapter staging
// textures (the FP16 generic0 twin at the seam, the back buffer before Present), mapped once after the last one and
// written as raw files with a JSON index of the jitter each frame carried.
struct Burst {
    struct Frame {
        ComPtr<ID3D11Texture2D> world, presented;
        uint32_t engine_frame{};
        JitterState jitter{};
        float mip_bias{};
        uint64_t fp_jittered{};
    };
    bool enabled{}, armed{}, finished{}, written{};
    uint32_t wanted{}, wait{};
    uint64_t start{}, fp_at_prepare{};
    BurstGate gate;   // loading screen / menu / no-world frames hold and restart the burst (sr_jitter.h)
    std::wstring directory;
    std::vector<Frame> frames;
    std::string result{"not started"};
};
ComPtr<ID3D11Texture2D> staging_copy(ID3D11DeviceContext* ctx, void* source) noexcept {
    if (!ctx || !source) return {};
    ComPtr<ID3D11Texture2D> texture;
    if (FAILED(static_cast<IUnknown*>(source)->QueryInterface(IID_PPV_ARGS(&texture)))) return {};
    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    if (desc.SampleDesc.Count != 1) return {};
    desc.MipLevels = desc.ArraySize = 1;
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    desc.MiscFlags = 0;
    ComPtr<ID3D11Device> device;
    ctx->GetDevice(&device);
    ComPtr<ID3D11Texture2D> copy;
    if (!device || FAILED(device->CreateTexture2D(&desc, nullptr, &copy))) return {};
    ctx->CopySubresourceRegion(copy.Get(), 0, 0, 0, 0, texture.Get(), 0, nullptr);
    return copy;
}

// ---- the D3D11/engine host behind the swap controller --------------------------------------------------------
struct Host;
// Stages 4 and 5 at the seam, after the restore: prepare the SR inputs, evaluate (Stage 5), reinsert or fall back to
// the bilinear pass. Defined with the process state below.
bool temporal_seam(Host&, ID3D11DeviceContext*, const UpscaleInputs&, const UpscaleViews&) noexcept;
struct Host final : SwapHost {
    std::array<Twin, slot_count> twins{};
    DepthTwin depth{};
    ScratchTarget scratch{};
    RoundTripPass pass;
    UpscalePass upscale_pass;
    TemporalTargets temporal{};
    ComPtr<ID3D11ShaderResourceView> motion_view;   // SRV of the motion stage's target, rebuilt when it changes
    void* motion_view_texture{};
    Capture* capture{};
    Burst* burst{};
    uint64_t last_drop_refs{};
    uint64_t twin_bytes{};
    bool read(const void* address, void* output, std::size_t size) noexcept override { return guarded_copy(output, address, size); }
    bool write(void* address, const void* input, std::size_t size) noexcept override { return guarded_copy(address, input, size); }
    ID3D11Device* device() noexcept { ID3D11Device* value{}; return read_global(hw_device_rva, value) ? value : nullptr; }
    ID3D11DeviceContext* context() noexcept { ID3D11DeviceContext* value{}; return read_global(hw_context_rva, value) ? value : nullptr; }
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
    bool upscale(const UpscaleInputs& in) noexcept override {
        auto* ctx = context();
        if (!in.generic0_twin || !in.position_twin || !in.generic1_twin || !in.generic0 || !in.position || !in.generic1) return false;
        if (capture && capture->active)
            take(ctx, "anomaly_seam_fp16_input", "SRStage=2/3: render-size FP16 generic0 twin at the S-B seam, engine low tonemap space, before the upscale",
                in.generic0_twin->surface);
        if (burst && burst->armed && !burst->frames.empty() && !burst->frames.back().world)
            burst->frames.back().world = staging_copy(ctx, in.generic0_twin->surface);
        const UpscaleViews views{static_cast<ID3D11ShaderResourceView*>(in.generic0_twin->srv),
            static_cast<ID3D11ShaderResourceView*>(in.position_twin->srv), static_cast<ID3D11ShaderResourceView*>(in.generic1_twin->srv),
            static_cast<ID3D11ShaderResourceView*>(in.depth_srv), in.render_width, in.render_height,
            static_cast<ID3D11RenderTargetView*>(in.generic0->rtv), static_cast<ID3D11RenderTargetView*>(in.position->rtv),
            static_cast<ID3D11RenderTargetView*>(in.generic1->rtv), static_cast<ID3D11DepthStencilView*>(in.engine_dsv),
            in.output_width, in.output_height};
        const bool ok = configured_options.sr_inputs() ? temporal_seam(*this, ctx, in, views) : upscale_pass.run(ctx, views);
        if (capture && capture->active) {
            take(ctx, "anomaly_seam_generic0", configured_options.stage == 5
                ? "SRStage=5: engine UNORM8 generic0 after the SR reinsertion (or the bilinear fallback) at output size; input to every post pass"
                : "SRStage=2/3/4: engine UNORM8 generic0 after the bilinear upscale to output size; input to every post pass",
                in.generic0->surface);
            capture->seam_taken = true;
        }
        return ok;
    }
    void set_viewport(uint32_t width, uint32_t height) noexcept override { set_full_viewport(context(), width, height); }
    bool upload_projection(const void* matrix) noexcept override { return call_set_projection(matrix); }
    bool set_mip_bias(float value) noexcept override { return call_set_mip_bias(value); }
    bool round_trip(const TargetFields& twin, const TargetFields& engine) noexcept override {
        auto* ctx = context();
        if (capture && capture->active)
            take(ctx, "anomaly_seam_fp16_input", "SRStage=1: FP16 generic0 twin at the S-B seam, engine low tonemap space, before the round trip",
                twin.surface);
        const bool ok = pass.run(ctx, static_cast<ID3D11ShaderResourceView*>(twin.srv), twin.width, twin.height,
            static_cast<ID3D11RenderTargetView*>(engine.rtv), engine.width, engine.height);
        if (capture && capture->active) {
            take(ctx, "anomaly_seam_generic0", "SRStage=1: engine UNORM8 generic0 after the f^-1/f round trip; input to every post pass",
                engine.surface);
            capture->seam_taken = true;
        }
        return ok;
    }
    void take(ID3D11DeviceContext* ctx, const char* name, const char* description, void* texture,
        rsf_dump_view view = RSF_DUMP_VIEW_RAW, float scale = 1.0f) noexcept {
        if (!dump_ready || !capture || !capture->active || !ctx || !texture) return;
        if (dump.texture_record(ctx, &capture->record, name, description, texture, view, scale)) ++capture->images;
        else ++capture->failed;
    }
};

// ---- process state --------------------------------------------------------------------------------------------
std::mutex guard;
Host host;
SwapController controller{host};
Capture capture;
Burst burst;
HookSet hooks;

// Stages 4 and 5. The main-view camera read at this frame's H1 (Device.mView and the still unjittered mProject), the
// previous main-view frame's, whether the step between them is a cut, and the time between the two H1s.
TemporalDriver driver;
ViewCamera frame_camera{}, previous_camera{};
bool frame_cut{};
float frame_seconds{};
int64_t last_main_qpc{};

// Stage 3 jitter: the main-view sequence and what was published at the last H1.
JitterSequence sequence;
JitterState main_state{}, published{};
std::atomic<float> published_uv[4]{};
bool jitter_enabled{};          // SRStage=3, SRJitter=1 and the rmNear slot is live
uint64_t no_world_mark{~0ull};  // frames_without_world at the last main-view H1 (a change is a history break)
void publish(const JitterState& state) noexcept {
    published = state;
    const auto uv = jitter_uv(state);
    published_uv[0].store(uv.current_u, std::memory_order_relaxed);
    published_uv[1].store(uv.current_v, std::memory_order_relaxed);
    published_uv[2].store(uv.previous_u, std::memory_order_relaxed);
    published_uv[3].store(uv.previous_v, std::memory_order_relaxed);
}

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
double reported_world_ms[2]{}, reported_seam_ms{};
uint64_t reported_world_samples[2]{}, reported_seam_samples{};
std::atomic<bool> hooks_active{};
std::array<void*, 4> originals{};
uint64_t session{};
uint64_t world_frames{}, frames{}, frames_without_world{};
bool prepared_this_frame{};
SwapCounters reported{};
uint64_t reported_frames{}, reported_without_world{};
float current_scale{default_scale};
uint64_t scale_generation{~0ull};
ScalePlan last_plan{};
using Unary = void(__fastcall*)(void*);

void build_record(uint32_t width, uint32_t height, uint32_t render_width, uint32_t render_height) noexcept {
    auto& r = capture.record;
    r = {};
    r.struct_size = sizeof(r);
    r.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    uint32_t engine_frame{};
    read_global(device_frame_rva, engine_frame);
    r.frame_id = static_cast<rsf_frame_id>(engine_frame) + 1;   // never RSF_FRAME_ID_NONE
    r.session_id = session;
    r.view_id = 0;
    r.resource_generation = static_cast<uint32_t>(controller.twin_generation());
    r.phase = RSF_PHASE_RENDER_SUBMIT;
    r.screen = RSF_SCREEN_UNKNOWN;
    r.output_width = width;
    r.output_height = height;
    r.render_width = render_width;
    r.render_height = render_height;
}
void capture_abandon() noexcept {
    if (!capture.active) return;
    dump.complete(&capture.record, E_FAIL);
    capture.active = false;
    ++capture.failed;
}
// At H1, on a main-view frame: arm the next scheduled capture, or join an F10 request.
void capture_arm(const FrameFacts& facts) noexcept {
    if (!dump_ready) return;
    capture_abandon();
    capture.seam_taken = false;
    dump.poll();
    const bool scheduled = capture.remaining && world_frames >= capture.next && !loading_screen_active();   // F10 is the user's call
    if (scheduled) dump.request();
    if (!dump.requested()) return;
    const bool stage2 = controller.window_open() && controller.stage() == 2;   // this frame renders the world at render size
    build_record(facts.device_width, facts.device_height, stage2 ? facts.render_width : facts.device_width,
        stage2 ? facts.render_height : facts.device_height);
    if (!dump.begin_record(&capture.record, 0)) return;
    capture.active = true;
    ++capture.taken;
    if (scheduled) { --capture.remaining; capture.next = world_frames + configured_options.capture_interval; }
}

FrameFacts gather(void* self) noexcept {
    FrameFacts facts{};
    std::byte* target{};
    if (read_global(render_implementation_rva + render_target_offset, target) && target == self)
        facts.render_target = target;   // phase_scene_prepare's `this` must be CRender::Target
    facts.backend = image + backend_rva;
    read_global(hw_device_rva, facts.device);
    NativeObservation observation{sizeof(observation), observation_abi};
    if (rsf_anomaly_observation(&observation) == RSF_OK) facts.generation = observation.resets;
    read_global(device_width_rva, facts.device_width);
    read_global(device_height_rva, facts.device_height);
    const auto view = geometry::current_render_view(image);
    facts.scope = !view || *view == temporal::ViewId::Scope;   // unreadable counts as not eligible
    uint32_t options3{};
    facts.msaa = !read_global(render_options_word3_rva, options3) || (options3 & dx10_msaa_bit);
    if (!configured_options.world_window()) make_plan(facts.device_width, facts.device_height, 1.0f, last_plan);
    if (configured_options.world_window()) {
        // A device reset (vid_restart, resolution change) re-reads SRScale, so a new value applies without a restart.
        if (facts.generation != scale_generation) {
            if (scale_generation != ~0ull && !configured_options.ini.empty()) {
                const float scale = read_scale(configured_options.ini.c_str());
                if (scale != current_scale) {
                    char text[120];
                    std::snprintf(text, sizeof(text), "Anomaly SR stage %u: SRScale %.3f -> %.3f after a device reset.",
                        configured_options.stage, current_scale, scale);
                    say(text);
                    current_scale = scale;
                }
            }
            scale_generation = facts.generation;
        }
        ScalePlan plan{};
        if (make_plan(facts.device_width, facts.device_height, current_scale, plan)) {
            facts.render_width = plan.render_width;
            facts.render_height = plan.render_height;
            last_plan = plan;
        }
        // Stage 5: the render size the selected backend plans for this output at the selected quality. SRScale stays
        // the fallback while no backend plans (not started, refused, or planning for another output size).
        uint32_t planned_width{}, planned_height{};
        if (configured_options.stage == 5 && driver.planned_render(facts.device_width, facts.device_height, planned_width, planned_height) &&
            make_plan_for_render(facts.device_width, facts.device_height, planned_width, planned_height, plan)) {
            facts.render_width = plan.render_width;
            facts.render_height = plan.render_height;
            last_plan = plan;
        }
        facts.stage = 2;
        facts.device_size = image + device_width_rva;
        facts.base_zb = image + hw_base_zb_rva;
        facts.base_rt = image + hw_base_rt_rva;
        facts.ssao_viewport = image + ssao_viewport_rva;
        facts.ssao_viewport_guard = image + ssao_viewport_guard_rva;
    }
    if (configured_options.temporal()) {
        facts.device_view = image + device_view_rva;
        facts.device_project = image + device_project_rva;
        facts.device_full_transform = image + device_full_transform_rva;
        facts.mip_bias = configured_options.mip_bias;
        facts.mip_bias_delta = window_mip_bias_delta(facts.render_width, facts.device_width, configured_options.mip_bias_offset);
        facts.sampler_bias = image + sampler_cache_rva + sampler_mip_lod_bias_offset;
    }
    return facts;
}

// Stage 3, before controller.prepare: the main-view sample for this frame. Scope frames take none.
JitterSample choose_jitter(FrameFacts& facts, bool& reset) noexcept {
    reset = false;
    if (!configured_options.temporal() || facts.scope || !jitter_enabled) return {};
    const bool continuous = no_world_mark == frames_without_world;   // no menu/loading frame since the last main H1
    no_world_mark = frames_without_world;
    const auto sample = sequence.next({facts.render_width, facts.render_height, facts.device_width, facts.device_height,
        facts.generation}, continuous);
    reset = sequence.last_was_reset();
    facts.jitter = true;
    facts.jitter_x = sample.x;
    facts.jitter_y = sample.y;
    return sample;
}
// Stage 3, after controller.prepare: what the motion stage and the SR pass see for this frame.
void publish_jitter(const FrameFacts& facts, const JitterSample& sample, bool reset) noexcept {
    if (!configured_options.temporal()) return;
    const bool applied = controller.window_open() && controller.jitter_applied();
    const uint32_t width = applied ? controller.render_width() : facts.render_width;
    const uint32_t height = applied ? controller.render_height() : facts.render_height;
    if (facts.scope) { publish(next_state(main_state, true, false, sample, width, height, false)); return; }
    main_state = next_state(main_state, false, applied, sample, width, height, reset);
    publish(main_state);
}
// Stage 3 burst: arm a consecutive capture frame at H1 (after publish_jitter).
void burst_arm(const FrameFacts& facts) noexcept {
    if (!burst.enabled || burst.finished || facts.scope) return;
    const auto verdict = burst.gate.frame(loading_screen_active(), frames_without_world, !burst.frames.empty());
    if (verdict.discard && !burst.finished) { burst.frames.clear(); burst.armed = false; }
    if (!verdict.capture || !controller.window_open() || world_frames < burst.start) return;
    if (burst.frames.size() >= burst.wanted) return;
    Burst::Frame frame{};
    uint32_t engine_frame{};
    read_global(device_frame_rva, engine_frame);
    frame.engine_frame = engine_frame;
    frame.jitter = published;
    frame.mip_bias = controller.mip_bias_applied() ? controller.window_mip_bias() : controller.saved_mip_bias();
    burst.frames.push_back(std::move(frame));
    burst.fp_at_prepare = controller.counters().fp_jittered;
    burst.armed = true;
}

// Stages 4 and 5, at a main-view H1 before the jitter is applied: this frame's camera, the cut test against the
// previous main-view frame, and the time between them.
void read_main_camera() noexcept {
    previous_camera = frame_camera;
    ViewCamera camera{};
    camera.valid = read_global(device_view_rva, camera.view) && read_global(device_project_rva, camera.projection);
    for (const float value : camera.view) camera.valid = camera.valid && std::isfinite(value);
    for (const float value : camera.projection) camera.valid = camera.valid && std::isfinite(value);
    frame_camera = camera;
    frame_cut = camera.valid && previous_camera.valid &&
        camera_cut(pose_of(previous_camera.view), pose_of(camera.view), configured_options.cut);
    LARGE_INTEGER now{}, frequency{};
    QueryPerformanceCounter(&now);
    QueryPerformanceFrequency(&frequency);
    frame_seconds = last_main_qpc && frequency.QuadPart ? float(double(now.QuadPart - last_main_qpc) / double(frequency.QuadPart)) : 0.0f;
    if (!(frame_seconds > 0.0f) || frame_seconds > 1.0f) frame_seconds = 0.0f;
    last_main_qpc = now.QuadPart;
}

// Stages 4 and 5 at the S-B seam, after the engine fields are restored (section 12). Inputs, in render pixels:
//   colour  f^-1 of the FP16 generic0 twin (exposure-scaled linear radiance, section 2.6 default)
//   depth   the R24G8 depth twin, standard device depth (section 3, D1)
//   motion  this frame's main-view motion target with the first-person rule, validity in channel 2
// Stage 5 then submits one pass to the shared pipeline and re-tonemaps its output into the engine's generic0 (with the
// nearest rt_Position, bilinear generic1 and nearest depth of the Stage 2 pass). Every refusal draws the Stage 2
// bilinear pass instead, so the engine always receives a complete world image.
std::string utf8(const std::wstring& value);
bool dump_directory(std::wstring& directory);
bool temporal_seam(Host& h, ID3D11DeviceContext* ctx, const UpscaleInputs& in, const UpscaleViews& views) noexcept {
    const auto bilinear = [&](bool) { driver.count_bilinear(); return h.upscale_pass.run(ctx, views); };
    auto* d3d = h.device();
    if (!ctx || !d3d || !h.upscale_pass.temporal_ready() || !h.depth.texture) { driver.count(Refusal::Prepare); return bilinear(true); }
    const uint32_t rw = in.render_width, rh = in.render_height, ow = in.output_width, oh = in.output_height;
    const bool evaluate = configured_options.stage == 5 && driver.enabled();
    const uint32_t pipeline_format = evaluate ? driver.output_format() : 0u;
    const auto format = pipeline_format ? static_cast<DXGI_FORMAT>(pipeline_format) : DXGI_FORMAT_R16G16B16A16_FLOAT;
    auto& t = h.temporal;
    if (!t.matches(d3d, rw, rh, ow, oh, format)) {
        const auto error = make_temporal_targets(d3d, rw, rh, ow, oh, format, t);
        char text[160];
        std::snprintf(text, sizeof(text), "Anomaly SR stage %u: SR input targets %s at render %ux%u, output %ux%u (format %u).",
            configured_options.stage, error == TwinError::None ? "created" : "refused", rw, rh, ow, oh, unsigned(format));
        say(text);
        if (error != TwinError::None) { driver.count(Refusal::Prepare); return bilinear(true); }
    }
    // The motion target the main view's G-buffer window wrote this frame, at render size.
    const auto target = motion::frame_target();
    uint32_t engine_frame{};
    read_global(device_frame_rva, engine_frame);
    ID3D11ShaderResourceView* motion_srv = t.empty_motion_srv.Get();
    bool motion_present = false;
    if (target.texture && target.written && target.main && target.frame == engine_frame && target.width == rw && target.height == rh) {
        if (h.motion_view_texture != target.texture) {
            h.motion_view.Reset();
            h.motion_view_texture = target.texture;
            if (FAILED(d3d->CreateShaderResourceView(static_cast<ID3D11Texture2D*>(target.texture), nullptr, &h.motion_view)))
                h.motion_view.Reset();
        }
        if (h.motion_view) { motion_srv = h.motion_view.Get(); motion_present = true; }
    }
    const PrepareViews prepare{views.colour, motion_srv, views.depth, rw, rh, t.colour_rtv.Get(), t.motion_rtv.Get(),
        first_person_depth_limit, configured_options.first_person_zero};
    if (!h.upscale_pass.prepare(ctx, prepare)) { driver.count(Refusal::Prepare); return bilinear(!motion_present); }
    driver.count_prepared(!motion_present);
    h.take(ctx, "anomaly_sr_color", "SRStage=4/5 SR colour: render-size f^-1 of the FP16 generic0 twin, exposure-scaled linear",
        t.colour.Get(), RSF_DUMP_VIEW_HDR, 1.0f);
    h.take(ctx, "anomaly_sr_depth", "SRStage=4/5 SR depth: render-size R24G8 depth twin, standard device depth (first person in [0, 0.02])",
        h.depth.texture.Get(), RSF_DUMP_VIEW_DEPTH, 1.0f);
    h.take(ctx, "anomaly_sr_motion", "SRStage=4/5 SR motion: current-minus-previous UV (Y down), validity in Z, first-person rule applied (UV x20)",
        t.motion.Get(), RSF_DUMP_VIEW_DENSE_MOTION, 20.0f);
    if (motion_present)
        h.take(ctx, "anomaly_motion_target", "Motion RTV2 as the G-buffer wrote it (raw RGBA16F)", target.texture, RSF_DUMP_VIEW_RAW, 1.0f);
    if (!evaluate) return bilinear(!motion_present);
    PassInputs pass{};
    pass.native_frame = published.frame;
    pass.generation = static_cast<uint32_t>(controller.twin_generation());
    pass.render_width = rw; pass.render_height = rh; pass.output_width = ow; pass.output_height = oh;
    pass.current = frame_camera;
    pass.previous = previous_camera;
    pass.jitter = published;
    pass.frame_time_seconds = frame_seconds;
    pass.color = t.colour.Get();
    pass.color_output = t.output.Get();
    pass.depth = h.depth.texture.Get();
    pass.motion = t.motion.Get();
    pass.motion_missing = !motion_present;
    pass.exposure = configured_options.exposure_auto ? nullptr : t.exposure.Get();
    ResetFacts facts{};
    facts.sequence = published.reset;
    facts.cut = frame_cut;
    // A captured frame also asks the pipeline for its own guides: the decoded and the dense motion it submitted, the
    // depth and colour as the backend received them, and the frame constants, beside the core dump record.
    if (h.capture && h.capture->active) try {
        std::wstring directory;
        if (dump_directory(directory)) {
            wchar_t name[96];
            std::swprintf(name, 96, L"\\sr-pipeline-%016llx", static_cast<unsigned long long>(session));
            directory += name;
            CreateDirectoryW(directory.c_str(), nullptr);
            char file[64];
            std::snprintf(file, sizeof(file), "\\frame_%llu", static_cast<unsigned long long>(h.capture->record.frame_id));
            if (driver.request_dump(utf8(directory) + file)) {
                std::string text = "Anomaly SR stage 5 capture: pipeline guides requested at " + utf8(directory) + file;
                say(text.c_str());
            }
        }
    } catch (...) {}
    if (!driver.evaluate(ctx, pass, facts)) return bilinear(!motion_present);
    h.take(ctx, "anomaly_sr_output", "SRStage=5 reconstruction at output size, linear (before the stock tonemap curve)",
        t.output.Get(), RSF_DUMP_VIEW_HDR, 1.0f);
    if (!h.upscale_pass.reinsert(ctx, views, t.output_srv.Get())) { driver.count(Refusal::Reinsert); return bilinear(!motion_present); }
    return true;
}

void __fastcall prepare_hook(void* self) {
    {
        std::lock_guard lock(guard);
        // A window still open here (never expected) is closed first, so Device.dwWidth reads the output size again.
        controller.close_stale();
        gpu_timer.begin(host.device(), host.context());
        auto facts = gather(self);
        if (configured_options.sr_inputs() && !facts.scope) read_main_camera();
        bool reset{};
        const auto sample = choose_jitter(facts, reset);
        prepared_this_frame = true;
        controller.prepare(facts);
        publish_jitter(facts, sample, reset);
        if (!facts.scope) {
            ++world_frames;
            if (configured_options.capture) capture_arm(facts);
            burst_arm(facts);
        }
    }
    reinterpret_cast<Unary>(originals[0])(self);
    std::lock_guard lock(guard);
    controller.prepared();   // Stage 2: the viewport now describes the render-size targets
}
void seam(void* self) noexcept {
    std::lock_guard lock(guard);
    if (controller.window_open()) {
        gpu_timer.seam(host.context(), true);
        controller.seam();
        gpu_timer.finish(host.context());
        return;
    }
    gpu_timer.seam(host.context(), false);
    gpu_timer.finish(host.context());
    controller.seam();   // counted as a pass-through
    // Stock capture: the engine generic0 as the post chain receives it, once per frame at the first seam call.
    if (capture.active && !capture.seam_taken) {
        std::byte* target{};
        std::byte* crt{};
        void* surface{};
        if (read_global(render_implementation_rva + render_target_offset, target) && target == self &&
            guarded_copy(&crt, target + 0x3a8, sizeof(crt)) && crt && guarded_copy(&surface, crt + crt_surface_offset, sizeof(surface)))
            host.take(host.context(), "anomaly_seam_generic0",
                controller.refused() || configured_options.stage == 0
                    ? "Stock (no swap this frame): engine UNORM8 generic0 at the S-B seam; input to every post pass"
                    : "SRStage frame without a window (skipped): stock engine UNORM8 generic0 at the S-B seam",
                surface);
        capture.seam_taken = true;
    }
}
void __fastcall sunshafts_hook(void* self) { seam(self); reinterpret_cast<Unary>(originals[2])(self); }
void __fastcall blur_hook(void* self) { seam(self); reinterpret_cast<Unary>(originals[3])(self); }
void __fastcall combine_hook(void* self) {
    reinterpret_cast<Unary>(originals[1])(self);
    std::lock_guard lock(guard);
    const bool was_refused = controller.refused();
    controller.combine_end();
    if (!was_refused && controller.refused()) {
        std::string text = "Anomaly SR stage refused further swaps: ";
        text += controller.refusal();
        say(text.c_str());
    }
}
void rm_near_callback(void*) noexcept {
    std::lock_guard lock(guard);
    controller.rm_near();
}

bool bytes_match(const std::byte* address, const char* hex) noexcept {
    const auto digit = [](char c) { return c <= '9' ? c - '0' : c - 'a' + 10; };
    for (std::size_t i = 0; hex[2 * i] && hex[2 * i + 1]; ++i) {
        std::byte value{};
        if (!guarded_copy(&value, address + i, 1) || value != static_cast<std::byte>(digit(hex[2 * i]) * 16 + digit(hex[2 * i + 1])))
            return false;
    }
    return true;
}
static_assert(device_height_rva == device_width_rva + sizeof(uint32_t), "Device.dwHeight follows dwWidth");
// Stage 3 calls R_xforms::set_P and SetMipLODBias, and relies on every first-person projection site uploading with
// set_P and then calling rmNear (the shared slot). Each must still be the verified code.
bool stage3_guards_match(std::byte* module, uint32_t image_size, const char** failed) noexcept {
    const auto& set_p = sr_function_sites[site_set_p];
    const auto& set_bias = sr_function_sites[site_set_mip_lod_bias];
    const auto& rm_near = sr_function_sites[site_rm_near];
    for (const auto& guard_site : sr_unique_guards)
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
    for (const auto& site : sr_first_person_sites) {
        // `call R_xforms::set_P`, then within 0x120 bytes `call [rax+0x1b8]` (vtable slot 55) or `call CRender::rmNear`.
        if (!rel_call_to(site.set_p_call, set_p.rva) || site.rm_near_call <= site.set_p_call ||
            site.rm_near_call - site.set_p_call >= 0x120 ||
            !(bytes_match(module + site.rm_near_call, "ff90b8010000") || rel_call_to(site.rm_near_call, rm_near.rva))) {
            *failed = site.name; return false;
        }
    }
    return true;
}
bool guards_match(std::byte* module, uint32_t image_size, const char** failed) noexcept {
    // Stage 2 writes the SSAO helper's static viewport: its code, which encodes both data addresses, must match.
    if (configured_options.world_window() && !bytes_match(module + ssao_set_viewport_rva, ssao_set_viewport_code)) {
        *failed = "set_viewport (r4_rendertarget_phase_ssao.cpp:3)"; return false;
    }
    if (configured_options.temporal() && !stage3_guards_match(module, image_size, failed)) return false;
    // rmNear is not checked here: the geometry producer verified and patched it, and Stage 1 reaches it only through
    // the shared slot.
    for (const auto index : {site_phase_scene_prepare, site_phase_combine, site_phase_sunshafts, site_phase_blur}) {
        const auto& site = sr_function_sites[index];
        std::array<std::byte, 16> bytes{};
        if (site.rva > image_size - bytes.size() || !guarded_copy(bytes.data(), module + site.rva, bytes.size()) || bytes != site.expected) {
            *failed = site.name; return false;
        }
    }
    for (const auto& guard_site : sr_unique_guards) {
        if (std::string_view{guard_site.name} != "CRenderTarget::phase_blur") continue;
        if (!bytes_match(module + guard_site.rva, guard_site.expected_hex)) { *failed = guard_site.name; return false; }
    }
    for (const auto& site : sr_call_sites) {
        std::array<std::byte, 16> bytes{};
        if (!guarded_copy(bytes.data(), module + site.rva, bytes.size()) || bytes != site.expected) { *failed = site.name; return false; }
    }
    return true;
}
std::string utf8(const std::wstring& value) {
    if (value.empty()) return {};
    const int size = WideCharToMultiByte(CP_UTF8, 0, value.c_str(), int(value.size()), nullptr, 0, nullptr, nullptr);
    std::string result(size > 0 ? size : 0, '\0');
    if (size > 0) WideCharToMultiByte(CP_UTF8, 0, value.c_str(), int(value.size()), result.data(), size, nullptr, nullptr);
    return result;
}
bool dump_directory(std::wstring& directory) {
    std::wstring root(32768, L'\0');
    const DWORD custom = GetEnvironmentVariableW(L"RSF_DUMP_DIR", root.data(), 32768);
    if (custom && custom < 32768) { directory.assign(root.c_str()); return true; }
    const DWORD size = GetEnvironmentVariableW(L"LOCALAPPDATA", root.data(), 32768);
    if (!size || size >= 32768) return false;
    directory = std::wstring(root.c_str()) + L"\\ReScaleFrame\\Anomaly";
    return true;
}
void bind_dump() noexcept try {
    const HMODULE runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    if (!runtime || !dump.bind(runtime)) { say("Anomaly SR stage capture refused: Runtime frame dump exports missing."); return; }
    std::wstring directory;
    if (!dump_directory(directory)) { say("Anomaly SR stage capture refused: LOCALAPPDATA unavailable."); return; }
    dump.configure(utf8(directory).c_str(), "Anomaly", dx11_hash, log_fn, log_user);
    dump_ready = true;
    // Stage 3 replaces the scheduled single-frame captures with the consecutive burst; F10 still takes one.
    capture.remaining = configured_options.stage == 3 ? 0 : configured_options.capture_count;
    capture.next = configured_options.capture_start;
    host.capture = &capture;
} catch (...) { dump_ready = false; }
void bind_burst() noexcept try {
    std::wstring directory;
    if (!dump_directory(directory)) { say("Anomaly SR stage 3 burst refused: LOCALAPPDATA unavailable."); return; }
    wchar_t name[64];
    std::swprintf(name, 64, L"\\sr-jitter-%016llx", static_cast<unsigned long long>(session));
    burst = {};
    burst.directory = directory + name;
    burst.wanted = configured_options.capture_count;
    burst.start = configured_options.capture_start;
    burst.frames.reserve(burst.wanted);
    burst.enabled = burst.wanted > 0;
    host.burst = &burst;
} catch (...) { burst.enabled = false; }

// Raw rows (tightly packed) of a mapped staging texture.
bool write_raw(const std::wstring& path, ID3D11DeviceContext* ctx, ID3D11Texture2D* texture, uint32_t& width, uint32_t& height,
    uint32_t& format, uint32_t& texel) noexcept {
    if (!texture) return false;
    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    width = desc.Width;
    height = desc.Height;
    format = desc.Format;
    texel = desc.Format == DXGI_FORMAT_R16G16B16A16_FLOAT ? 8 : 4;
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(ctx->Map(texture, 0, D3D11_MAP_READ, 0, &mapped))) return false;
    FILE* file{};
    bool ok = _wfopen_s(&file, path.c_str(), L"wb") == 0 && file;
    for (uint32_t row = 0; ok && row < desc.Height; ++row)
        ok = std::fwrite(static_cast<const std::byte*>(mapped.pData) + std::size_t(row) * mapped.RowPitch, texel, desc.Width, file) == desc.Width;
    if (file) std::fclose(file);
    ctx->Unmap(texture, 0);
    return ok;
}
// After the last frame and a short GPU drain: map every copy once, write world_NN.bin / final_NN.bin and burst.json.
void burst_write(ID3D11DeviceContext* ctx) noexcept try {
    burst.written = true;
    if (!ctx) { burst.result = "no device context"; return; }
    CreateDirectoryW(burst.directory.c_str(), nullptr);
    std::string json = "{\n  \"schema\": \"rsf-anomaly-sr-jitter-burst-1\",\n  \"convention\": \"jitter in render pixels, +X right, +Y down; "
        "the rasterized world moves by (+x, +y). world_NN.bin: render-size FP16 RGBA generic0 twin at the S-B seam (engine low "
        "tonemap space), rows packed. final_NN.bin: back buffer before Present, rows packed.\",\n";
    char line[640];
    std::snprintf(line, sizeof(line), "  \"session\": \"%016llx\", \"scale\": %.4f, \"mip_bias_offset\": %.3f, \"mip_bias\": %s,\n  \"frames\": [\n",
        static_cast<unsigned long long>(session), double(current_scale), double(configured_options.mip_bias_offset),
        configured_options.mip_bias ? "true" : "false");
    json += line;
    uint32_t written = 0;
    for (std::size_t i = 0; i < burst.frames.size(); ++i) {
        auto& f = burst.frames[i];
        wchar_t world_name[32], final_name[32];
        std::swprintf(world_name, 32, L"\\world_%02zu.bin", i);
        std::swprintf(final_name, 32, L"\\final_%02zu.bin", i);
        uint32_t ww{}, wh{}, wf{}, wt{}, fw{}, fh{}, ff{}, ft{};
        const bool world_ok = write_raw(burst.directory + world_name, ctx, f.world.Get(), ww, wh, wf, wt);
        const bool final_ok = write_raw(burst.directory + final_name, ctx, f.presented.Get(), fw, fh, ff, ft);
        written += world_ok;
        const auto& j = f.jitter;
        std::snprintf(line, sizeof(line),
            "    {\"index\": %zu, \"engine_frame\": %u, \"jitter_frame\": %llu, \"sequence_index\": %u, \"phase_count\": %u, "
            "\"applied\": %s, \"reset\": %s, \"jitter_px\": [%.6f, %.6f], \"previous_jitter_px\": [%.6f, %.6f], \"render\": [%u, %u], "
            "\"mip_bias\": %.4f, \"fp_jittered\": %llu, \"loading_screen\": false, \"world\": %s, \"world_size\": [%u, %u], \"world_dxgi\": %u, "
            "\"final\": %s, \"final_size\": [%u, %u], \"final_dxgi\": %u}%s\n",
            i, f.engine_frame, static_cast<unsigned long long>(j.frame), j.sequence_index, j.phase_count, j.applied ? "true" : "false",
            j.reset ? "true" : "false", double(j.current_x), double(j.current_y), double(j.previous_x), double(j.previous_y),
            j.render_width, j.render_height, double(f.mip_bias), static_cast<unsigned long long>(f.fp_jittered),
            world_ok ? "true" : "false", ww, wh, wf, final_ok ? "true" : "false", fw, fh, ff, i + 1 < burst.frames.size() ? "," : "");
        json += line;
        f.world.Reset();
        f.presented.Reset();
    }
    json += "  ]\n}\n";
    FILE* file{};
    if (_wfopen_s(&file, (burst.directory + L"\\burst.json").c_str(), L"wb") == 0 && file) {
        std::fwrite(json.data(), 1, json.size(), file);
        std::fclose(file);
    }
    std::snprintf(line, sizeof(line), "%u of %zu world frames written to %s", written, burst.frames.size(), utf8(burst.directory).c_str());
    burst.result = line;
    say(("Anomaly SR stage 3 jitter burst: " + burst.result).c_str());
} catch (...) { burst.result = "write failed"; }
}

JitterState current_jitter() noexcept {
    std::lock_guard lock(guard);
    return published;
}
JitterUv current_jitter_uv() noexcept {
    return {published_uv[0].load(std::memory_order_relaxed), published_uv[1].load(std::memory_order_relaxed),
        published_uv[2].load(std::memory_order_relaxed), published_uv[3].load(std::memory_order_relaxed)};
}

float read_scale(const wchar_t* ini, bool* defaulted) noexcept {
    if (defaulted) *defaulted = true;
    if (!ini) return default_scale;
    wchar_t text[64]{};
    GetPrivateProfileStringW(L"Anomaly", L"SRScale", L"", text, 64, ini);
    wchar_t* end{};
    const double value = std::wcstod(text, &end);
    if (!text[0] || end == text) return default_scale;
    while (*end == L' ' || *end == L'\t') ++end;
    if (*end) return default_scale;   // trailing garbage: "0.67x"
    const float scale = clamp_scale(static_cast<float>(value));
    if (defaulted) *defaulted = !(value > 0.0) || !std::isfinite(value);
    return scale;
}

float read_mip_bias(const wchar_t* ini, bool* enabled) noexcept {
    if (enabled) *enabled = true;
    if (!ini) return default_mip_bias_offset;
    wchar_t text[64]{};
    GetPrivateProfileStringW(L"Anomaly", L"SRMipBias", L"", text, 64, ini);
    const wchar_t* begin = text;
    while (*begin == L' ' || *begin == L'\t') ++begin;
    if (_wcsnicmp(begin, L"off", 3) == 0) {
        const wchar_t* rest = begin + 3;
        while (*rest == L' ' || *rest == L'\t') ++rest;
        if (!*rest) { if (enabled) *enabled = false; return default_mip_bias_offset; }
    }
    wchar_t* end{};
    const double value = std::wcstod(begin, &end);
    if (!*begin || end == begin) return default_mip_bias_offset;
    while (*end == L' ' || *end == L'\t') ++end;
    if (*end || !std::isfinite(value)) return default_mip_bias_offset;
    return clamp_mip_bias_offset(static_cast<float>(value));
}

StageOptions read_options(const wchar_t* ini) noexcept try {
    StageOptions result{};
    if (!ini) return result;
    const auto stage = GetPrivateProfileIntW(L"Anomaly", L"SRStage", 0, ini);
    result.stage = stage >= 1 && stage <= 5 ? static_cast<uint32_t>(stage) : 0u;
    result.scale = read_scale(ini, &result.scale_defaulted);
    result.jitter = GetPrivateProfileIntW(L"Anomaly", L"SRJitter", 1, ini) != 0;
    result.mip_bias_offset = read_mip_bias(ini, &result.mip_bias);
    result.ini = ini;
    result.capture = GetPrivateProfileIntW(L"Anomaly", L"SRStageCapture", 0, ini) != 0;
    result.capture_start = GetPrivateProfileIntW(L"Anomaly", L"SRStageCaptureStart", 600, ini);
    result.capture_count = GetPrivateProfileIntW(L"Anomaly", L"SRStageCaptureCount", result.stage == 3 ? 8 : 3, ini);
    result.capture_interval = GetPrivateProfileIntW(L"Anomaly", L"SRStageCaptureInterval", 300, ini);
    const uint32_t most = result.stage == 3 ? 32 : 16;   // a Stage 3 burst can hold a whole 18- or 32-phase cycle
    if (result.capture_count > most) result.capture_count = most;
    if (!result.capture_interval) result.capture_interval = 1;
    // Stages 4 and 5 (section 12).
    wchar_t text[64]{};
    GetPrivateProfileStringW(L"Anomaly", L"SRExposure", L"constant", text, 64, ini);
    result.exposure_auto = !_wcsicmp(text, L"auto");
    result.first_person_zero = GetPrivateProfileIntW(L"Anomaly", L"SRFirstPersonZero", 1, ini) != 0;
    const auto read_float = [&](const wchar_t* key, float fallback, float low, float high) {
        wchar_t value[64]{};
        GetPrivateProfileStringW(L"Anomaly", key, L"", value, 64, ini);
        wchar_t* end{};
        const double parsed = std::wcstod(value, &end);
        if (!value[0] || end == value || !std::isfinite(parsed)) return fallback;
        return parsed < low ? low : parsed > high ? high : static_cast<float>(parsed);
    };
    result.cut.distance = read_float(L"SRCutDistance", 4.0f, 0.1f, 1000.0f);
    result.cut.degrees = read_float(L"SRCutAngle", 45.0f, 1.0f, 180.0f);
    result.require_signature = GetPrivateProfileIntW(L"Anomaly", L"SRRequireSignature", 0, ini) != 0;
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
    // The shared frontend keys the overlay persists in the same section; read here so SR applies without the overlay.
    SrSettings settings{};
    settings.enabled = GetPrivateProfileIntW(L"Anomaly", L"SR", 1, ini) != 0;
    settings.backend = GetPrivateProfileIntW(L"Anomaly", L"SRBackend", backend_dlss, ini);
    settings.quality = GetPrivateProfileIntW(L"Anomaly", L"SRQuality", 1, ini);
    settings.preset = GetPrivateProfileIntW(L"Anomaly", L"DLSSPreset", 0, ini);
    result.settings = sanitize(settings, SrSettings{});
    return result;
} catch (...) { return {}; }
void set_scale(float value) noexcept {
    std::lock_guard lock(guard);
    current_scale = clamp_scale(value);
}
void configure(const StageOptions& value, rsf_game_log_fn log, void* user) noexcept {
    std::lock_guard lock(guard);
    if (hooks_active.load()) return;
    configured_options = value;
    log_fn = log;
    log_user = user;
    if (value.sr_inputs()) {
        TemporalOptions temporal{};
        temporal.stage = value.stage;
        temporal.exposure_auto = value.exposure_auto;
        temporal.first_person_zero = value.first_person_zero;
        temporal.cut = value.cut;
        temporal.streamline = utf8(value.streamline_dir);
        temporal.fidelityfx = utf8(value.fidelityfx_dir);
        temporal.xess = utf8(value.xess_dir);
        temporal.require_signature = value.require_signature;
        temporal.initial = value.settings;
        driver.configure(temporal, log, user);
    }
    static constexpr OverlayLink link{&overlay_choices, &overlay_report, &request_settings};
    overlay_link.store(value.stage == 5 ? &link : nullptr, std::memory_order_release);
}
const StageOptions& options() noexcept { return configured_options; }
bool active() noexcept { return hooks_active.load(); }

bool install(std::byte* module, uint32_t image_size) noexcept try {
    if (!configured_options.selected()) return true;
    std::unique_lock lock(guard);
    if (hooks_active.load()) return true;
    if (!module || image_size != native_image_size) { say("Anomaly SR stage refused: image size differs from the verified build."); return false; }
    const char* failed = "unknown";
    // A re-start after quiesce finds our own detours in place of the guarded bytes; the first install verified them.
    if (!hooks.created() && !guards_match(module, image_size, &failed)) {
        std::string text = "Anomaly SR stage refused, game left stock: guard mismatch at ";
        text += failed;
        say(text.c_str());
        return false;
    }
    image = module;
    LARGE_INTEGER qpc{};
    QueryPerformanceCounter(&qpc);
    session = static_cast<uint64_t>(qpc.QuadPart) | 1u;
    controller.set_swap_enabled((configured_options.stage == 1 || configured_options.world_window()) &&
        (configured_options.stage != 5 || driver.enabled()));
    current_scale = clamp_scale(configured_options.scale);
    scale_generation = ~0ull;
    // Stage 3: the main projection is jittered here, the first-person ones only through the shared rmNear slot. Without
    // that slot (geometry hooks refused) the weapon would shimmer against the world, so jitter stays off.
    sequence.clear();
    main_state = {};
    publish({});
    no_world_mark = ~0ull;
    jitter_enabled = configured_options.temporal() && configured_options.jitter && geometry::geometry_hooks_active();
    if (configured_options.temporal()) {
        set_projection_fn = reinterpret_cast<SetProjection>(module + sr_function_sites[site_set_p].rva);
        set_mip_bias_fn = reinterpret_cast<SetMipBias>(module + sr_function_sites[site_set_mip_lod_bias].rva);
        if (configured_options.jitter && !jitter_enabled)
            say("Anomaly SR stage 3: jitter off, the rmNear slot is unavailable (geometry hooks refused); mip bias only.");
        if (configured_options.stage == 5 && !jitter_enabled)
            say("Anomaly SR stage 5: no jitter (SRJitter=0 or the rmNear slot is unavailable); every SR pass is refused and the seam stays bilinear.");
    }
    // Stage 5 borrows the carrier-loaded Runtime's SR pipeline; the pipeline itself starts at the first frame boundary.
    if (configured_options.stage == 5) driver.bind(GetModuleHandleW(L"ReScaleFrame.Runtime.dll"));
    frame_camera = previous_camera = {};
    frame_cut = false;
    last_main_qpc = 0;
    const std::array<HookSite, 4> sites{{
        {sr_function_sites[site_phase_scene_prepare].name, sr_function_sites[site_phase_scene_prepare].rva,
            sr_function_sites[site_phase_scene_prepare].expected, reinterpret_cast<void*>(&prepare_hook), &originals[0]},
        {sr_function_sites[site_phase_combine].name, sr_function_sites[site_phase_combine].rva,
            sr_function_sites[site_phase_combine].expected, reinterpret_cast<void*>(&combine_hook), &originals[1]},
        {sr_function_sites[site_phase_sunshafts].name, sr_function_sites[site_phase_sunshafts].rva,
            sr_function_sites[site_phase_sunshafts].expected, reinterpret_cast<void*>(&sunshafts_hook), &originals[2]},
        {sr_function_sites[site_phase_blur].name, sr_function_sites[site_phase_blur].rva,
            sr_function_sites[site_phase_blur].expected, reinterpret_cast<void*>(&blur_hook), &originals[3]}}};
    const bool installed = hooks.created() ? hooks.enable() : hooks.install(module, image_size, sites);
    if (!installed) { say("Anomaly SR stage refused: guarded detour installation failed; game left stock."); return false; }
    if (configured_options.stage == 1 || configured_options.world_window()) geometry::geometry_set_rm_near_callback(&rm_near_callback);
    hooks_active = true;
    lock.unlock();
    if (configured_options.capture) bind_dump();
    if (configured_options.capture && configured_options.stage == 3) bind_burst();
    char text[900];
    char scale[600];
    if (configured_options.sr_inputs())
        std::snprintf(scale, sizeof(scale),
            "%s; SRScale=%.3f%s%s; jitter=%s, mip bias %s (log2(render/output) %+.3f); SR inputs: linear colour, R24G8 depth twin, "
            "motion target (validity in Z, first-person zero %s); exposure %s; cut at %.2f m or %.1f deg",
            configured_options.stage == 5 ? "temporal SR through the shared RSF pipeline at the seam (bilinear on any refusal)"
                                          : "SR inputs prepared at the seam, bilinear output",
            current_scale, configured_options.scale_defaulted ? " (default)" : "",
            configured_options.stage == 5 ? " (fallback; the selected backend's quality plans the render size)" : "",
            jitter_enabled ? "on" : "off", configured_options.mip_bias ? "on" : "off", double(configured_options.mip_bias_offset),
            configured_options.first_person_zero ? "on" : "off", configured_options.exposure_auto ? "auto (backend)" : "constant 1.0",
            double(configured_options.cut.distance), double(configured_options.cut.degrees));
    else if (configured_options.stage == 3)
        std::snprintf(scale, sizeof(scale),
            "SRScale=%.3f%s, bilinear upscale at the seam; jitter=%s (Halton 2,3, ceil(8*(output/render)^2) phases), mip bias %s "
            "(log2(render/output) %+.3f), burst=%u consecutive frames from world frame %u",
            current_scale, configured_options.scale_defaulted ? " (default)" : "", jitter_enabled ? "on" : "off",
            configured_options.mip_bias ? "on" : "off", double(configured_options.mip_bias_offset),
            configured_options.capture ? configured_options.capture_count : 0u, configured_options.capture_start);
    else if (configured_options.stage == 2)
        std::snprintf(scale, sizeof(scale), "SRScale=%.3f%s, bilinear upscale at the seam", current_scale,
            configured_options.scale_defaulted ? " (default)" : "");
    else std::snprintf(scale, sizeof(scale), "scale 1.0");
    std::snprintf(text, sizeof(text),
        "Anomaly SR stage hooks installed: SRStage=%u SRStageCapture=%d (start=%u count=%u interval=%u) rmNear slot=%s; %s%s.",
        configured_options.stage, configured_options.capture ? 1 : 0, configured_options.capture_start, configured_options.capture_count,
        configured_options.capture_interval,
        configured_options.stage ? (geometry::geometry_hooks_active() ? "shared" : "unavailable (geometry hooks refused)") : "unused", scale,
        configured_options.stage == 5 ? "" : ", no SR provider is called");
    say(text);
    return true;
} catch (...) { return false; }

bool quiesce() noexcept {
    if (!hooks.created()) return true;
    geometry::geometry_set_rm_near_callback(nullptr);
    const bool disabled = hooks.disable();
    std::lock_guard lock(guard);
    controller.shutdown();
    gpu_timer = {};
    publish({});
    main_state = {};
    for (auto& frame : burst.frames) { frame.world.Reset(); frame.presented.Reset(); }
    burst.armed = false;
    if (dump_ready) { capture_abandon(); dump.cancel(session); }
    // Teardown: the pipeline releases its device reference and every vendor context before the engine's device goes.
    driver.stop();
    host.temporal = {};
    host.motion_view.Reset();
    host.motion_view_texture = nullptr;
    host.pass.reset();
    host.upscale_pass.reset();
    if (disabled) hooks_active = false;
    return disabled;
}

void frame_end() noexcept {
    if (!hooks_active.load(std::memory_order_acquire)) return;
    std::lock_guard lock(guard);
    controller.frame_end();
    if (configured_options.stage == 5) {
        // The drained boundary: this frame's SR work is submitted and Present has not run. Start the pipeline, follow a
        // device or output change and apply the shared settings here; the next H1 plans its render size from them.
        uint32_t width{}, height{};
        read_global(device_width_rva, width);
        read_global(device_height_rva, height);
        driver.boundary(host.device(), width, height);
        // SR=0: the world window stays closed and the frame renders stock at output size.
        if (!controller.refused()) controller.set_swap_enabled(driver.enabled());
    }
    ++frames;
    if (!prepared_this_frame) ++frames_without_world;   // menu, loading, first frame after reset
    prepared_this_frame = false;
    if (burst.armed) {
        // The burst frame armed at this frame's H1: the presented image, then the next frame (or the drain).
        burst.armed = false;
        if (loading_screen_active()) {
            // The loading screen appeared during this frame and may be the presented image: start the burst over.
            burst.frames.clear();
            burst.gate.interrupted();
        } else {
            auto& frame = burst.frames.back();
            frame.fp_jittered = controller.counters().fp_jittered - burst.fp_at_prepare;
            ID3D11RenderTargetView* base{};
            if (read_global(hw_base_rt_rva, base) && base) {
                ID3D11Resource* back{};
                base->GetResource(&back);
                if (back) { frame.presented = staging_copy(host.context(), back); back->Release(); }
            }
            if (burst.frames.size() >= burst.wanted) { burst.finished = true; burst.wait = 3; }
        }
    } else if (burst.finished && !burst.written && burst.wait-- == 0) burst_write(host.context());
    if (!capture.active) return;
    // The presented frame: the back buffer immediately before End's Present, with UI.
    ID3D11RenderTargetView* base{};
    if (read_global(hw_base_rt_rva, base) && base) {
        ID3D11Resource* back{};
        base->GetResource(&back);
        if (back) {
            host.take(host.context(), "final_color", "Back buffer before Present (world, post, HUD and UI)", back);
            back->Release();
        }
    }
    dump.complete(&capture.record, S_OK);
    capture.active = false;
}

void report(void (*emit)(void*, const char*), void* user) noexcept {
    if (!emit || !hooks_active.load()) return;
    SwapCounters now;
    uint64_t now_frames, now_without, twin_refs, bytes;
    Capture cap;
    const char* refusal;
    ScalePlan plan;
    float scale;
    JitterState jitter;
    uint64_t sequence_resets;
    float mip_window, mip_saved;
    bool mip_applied;
    std::string burst_state;
    struct { double world[2], seam; uint64_t samples[2], seam_samples, skipped; } timer{};
    {
        std::lock_guard lock(guard);
        jitter = main_state;
        sequence_resets = sequence.resets();
        mip_window = controller.window_mip_bias();
        mip_saved = controller.saved_mip_bias();
        mip_applied = controller.mip_bias_applied() || controller.counters().mip_applied;
        char text[96];
        std::snprintf(text, sizeof(text), "%zu/%u held=%llu restarts=%llu%s", burst.frames.size(), burst.wanted,
            static_cast<unsigned long long>(burst.gate.held()), static_cast<unsigned long long>(burst.gate.restarts()),
            loading_screen_active() ? " loading-screen" : "");
        burst_state = burst.enabled ? (burst.written ? burst.result : std::string(text)) : std::string("off");
        now = controller.counters();
        now_frames = frames; now_without = frames_without_world; twin_refs = host.last_drop_refs;
        cap = capture; refusal = controller.refusal();
        plan = last_plan; scale = current_scale; bytes = host.twin_bytes;
        if (auto* ctx = host.context(); ctx && gpu_timer.device) gpu_timer.collect(ctx);
        timer = {gpu_timer.world_ms[0], gpu_timer.world_ms[1], gpu_timer.seam_ms, gpu_timer.samples[0], gpu_timer.samples[1],
            gpu_timer.seam_samples, gpu_timer.skipped};
    }
    const auto average = [](double sum, double before, uint64_t count, uint64_t before_count) {
        return count > before_count ? (sum - before) / double(count - before_count) : 0.0;
    };
    const double stock_ms = average(timer.world[0], reported_world_ms[0], timer.samples[0], reported_world_samples[0]);
    const double swapped_ms = average(timer.world[1], reported_world_ms[1], timer.samples[1], reported_world_samples[1]);
    const double seam_ms = average(timer.seam, reported_seam_ms, timer.seam_samples, reported_seam_samples);
    const auto d = [&](uint64_t SwapCounters::*field) { return static_cast<unsigned long long>(now.*field - reported.*field); };
    const auto skip = [&](Skip s) { const auto i = static_cast<std::size_t>(s); return static_cast<unsigned long long>(now.skipped[i] - reported.skipped[i]); };
    const auto rest = [&](Restore r) { const auto i = static_cast<std::size_t>(r); return static_cast<unsigned long long>(now.restores[i] - reported.restores[i]); };
    char text[1024];
    std::snprintf(text, sizeof(text),
        "Anomaly SR stage window frames=%llu no_world=%llu prepare=%llu eligible=%llu swapped=%llu skipped off=%llu scope=%llu msaa=%llu "
        "layout=%llu twins=%llu | seams=%llu passthrough=%llu round_trips=%llu rt_failed=%llu | restores seam=%llu combine_end=%llu "
        "stale_prepare=%llu frame_end=%llu shutdown=%llu rollback=%llu | rm_near_in_window=%llu foreign=%llu write_faults=%llu | "
        "depth=%llu alloc_failed=%llu viewports=%llu ssao_viewport=%llu | gpu_world_ms swapped=%.3f (n=%llu) stock=%.3f (n=%llu) "
        "gpu_seam_ms=%.3f",
        static_cast<unsigned long long>(now_frames - reported_frames), static_cast<unsigned long long>(now_without - reported_without_world),
        d(&SwapCounters::prepares), d(&SwapCounters::eligible), d(&SwapCounters::swapped), skip(Skip::Off), skip(Skip::Scope),
        skip(Skip::Msaa), skip(Skip::Layout), skip(Skip::Twins), d(&SwapCounters::seams), d(&SwapCounters::seams_without_window),
        d(&SwapCounters::round_trips), d(&SwapCounters::round_trip_failures), rest(Restore::Seam), rest(Restore::CombineEnd),
        rest(Restore::StalePrepare), rest(Restore::FrameEnd), rest(Restore::Shutdown), rest(Restore::Rollback),
        d(&SwapCounters::rm_near_in_window), d(&SwapCounters::foreign_changes), d(&SwapCounters::write_faults), skip(Skip::Depth),
        d(&SwapCounters::allocation_failures), d(&SwapCounters::viewports), d(&SwapCounters::ssao_viewport_swaps), swapped_ms,
        static_cast<unsigned long long>(timer.samples[1] - reported_world_samples[1]), stock_ms,
        static_cast<unsigned long long>(timer.samples[0] - reported_world_samples[0]), seam_ms);
    emit(user, text);
    uint64_t restores{};
    for (const auto value : now.restores) restores += value;
    std::snprintf(text, sizeof(text),
        "Anomaly SR stage totals stage=%u swapped=%llu restores=%llu round_trips=%llu twin_generation=%llu twin_drops=%llu twin_refs_at_drop=%llu "
        "captures=%llu images=%llu capture_failed=%llu render=%ux%u output=%ux%u scale=%.3f twin_mib=%.1f refused=%s",
        configured_options.stage, static_cast<unsigned long long>(now.swapped), static_cast<unsigned long long>(restores),
        static_cast<unsigned long long>(now.round_trips), static_cast<unsigned long long>(now.twin_generations),
        static_cast<unsigned long long>(now.twin_drops), static_cast<unsigned long long>(twin_refs),
        static_cast<unsigned long long>(cap.taken), static_cast<unsigned long long>(cap.images), static_cast<unsigned long long>(cap.failed),
        configured_options.world_window() ? plan.render_width : plan.output_width,
        configured_options.world_window() ? plan.render_height : plan.output_height,
        plan.output_width, plan.output_height, configured_options.world_window() ? double(scale) : 1.0, double(bytes) / (1024.0 * 1024.0),
        refusal ? refusal : "no");
    emit(user, text);
    if (configured_options.temporal()) {
        std::snprintf(text, sizeof(text),
            "Anomaly SR stage 3 window jittered=%llu jitter_invalid=%llu fp_jittered=%llu fp_already=%llu fp_foreign=%llu "
            "projection_restores=%llu | mip applied=%llu restored=%llu unchanged=%llu | sequence frame=%llu index=%u/%u resets=%llu "
            "last=(%+.4f,%+.4f) previous=(%+.4f,%+.4f) applied=%d | mip window=%.3f user=%.3f %s | burst %s",
            d(&SwapCounters::jittered), d(&SwapCounters::jitter_invalid), d(&SwapCounters::fp_jittered), d(&SwapCounters::fp_already),
            d(&SwapCounters::fp_foreign), d(&SwapCounters::projection_restores), d(&SwapCounters::mip_applied),
            d(&SwapCounters::mip_restored), d(&SwapCounters::mip_unchanged), static_cast<unsigned long long>(jitter.frame),
            jitter.sequence_index, jitter.phase_count, static_cast<unsigned long long>(sequence_resets), double(jitter.current_x),
            double(jitter.current_y), double(jitter.previous_x), double(jitter.previous_y), jitter.applied ? 1 : 0, double(mip_window),
            double(mip_saved), mip_applied ? "" : "(never applied)", burst_state.c_str());
        emit(user, text);
    }
    if (configured_options.sr_inputs()) driver.report(emit, user);
    reported = now; reported_frames = now_frames; reported_without_world = now_without;
    reported_world_ms[0] = timer.world[0]; reported_world_ms[1] = timer.world[1]; reported_seam_ms = timer.seam;
    reported_world_samples[0] = timer.samples[0]; reported_world_samples[1] = timer.samples[1]; reported_seam_samples = timer.seam_samples;
}

// ---- the shared overlay (overlay_service.cpp) -------------------------------------------------------------------
namespace {
bool file_exists(const std::wstring& directory, const wchar_t* name) noexcept try {
    const auto path = std::filesystem::path(directory) / name;
    const DWORD attributes = GetFileAttributesW(path.c_str());
    return attributes != INVALID_FILE_ATTRIBUTES && !(attributes & FILE_ATTRIBUTE_DIRECTORY);
} catch (...) { return false; }
}
OverlayChoices overlay_choices() noexcept {
    OverlayChoices out{};
    const auto& o = configured_options;
    if (o.stage != 5) return out;
    // Offered when the runtime the backend loads is where the pipeline will look. Whether this GPU runs it is only known
    // once the pipeline starts; the report says so when it does not.
    if (file_exists(o.streamline_dir, L"sl.interposer.dll") && file_exists(o.streamline_dir, L"nvngx_dlss.dll"))
        out.backends |= 1u << backend_dlss;
    if (file_exists(o.fidelityfx_dir, L"amd_fidelityfx_upscaler_dx12.dll") && file_exists(o.fidelityfx_dir, L"amd_fidelityfx_loader_dx12.dll"))
        out.backends |= (1u << backend_fsr3) | (1u << backend_fsr4);
    if (file_exists(o.xess_dir, L"libxess.dll")) out.backends |= 1u << backend_xess;
    if (!out.backends) return out;
    out.qualities = adapter_qualities;
    out.preset = (out.backends >> backend_dlss) & 1u;
    out.defaults = o.settings;
    if (!((out.backends >> out.defaults.backend) & 1u))
        for (const auto backend : {backend_dlss, backend_xess, backend_fsr3, backend_fsr4})
            if ((out.backends >> backend) & 1u) { out.defaults.backend = backend; break; }
    return out;
}
int overlay_report(rsf_overlay_sr_report* report) noexcept {
    if (!report || configured_options.stage != 5 || !hooks_active.load(std::memory_order_acquire)) return 0;
    return driver.overlay_report(*report) ? 1 : 0;
}
void request_settings(const SrSettings& settings) noexcept {
    if (configured_options.stage == 5) driver.request(settings);
}
}
