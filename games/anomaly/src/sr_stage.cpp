// SPDX-License-Identifier: GPL-3.0-only
#include "sr_stage.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include "native_observer.h"
#include "sr_d3d11.h"
#include "sr_profile.h"
#include "sr_swap.h"
#include <rescaleframe/game_frame.h>
#include <rescaleframe/texture_dump.h>
#include <windows.h>
#include <atomic>
#include <cstdio>
#include <cmath>
#include <cstring>
#include <cwchar>
#include <mutex>
#include <string>
#include <string_view>

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

// ---- the D3D11/engine host behind the swap controller --------------------------------------------------------
struct Host final : SwapHost {
    std::array<Twin, slot_count> twins{};
    DepthTwin depth{};
    ScratchTarget scratch{};
    RoundTripPass pass;
    UpscalePass upscale_pass;
    Capture* capture{};
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
        last_drop_refs = refs;
    }
    bool upscale(const UpscaleInputs& in) noexcept override {
        auto* ctx = context();
        if (!in.generic0_twin || !in.position_twin || !in.generic1_twin || !in.generic0 || !in.position || !in.generic1) return false;
        if (capture && capture->active)
            take(ctx, "anomaly_seam_fp16_input", "SRStage=2: render-size FP16 generic0 twin at the S-B seam, engine low tonemap space, before the upscale",
                in.generic0_twin->surface);
        const UpscaleViews views{static_cast<ID3D11ShaderResourceView*>(in.generic0_twin->srv),
            static_cast<ID3D11ShaderResourceView*>(in.position_twin->srv), static_cast<ID3D11ShaderResourceView*>(in.generic1_twin->srv),
            static_cast<ID3D11ShaderResourceView*>(in.depth_srv), in.render_width, in.render_height,
            static_cast<ID3D11RenderTargetView*>(in.generic0->rtv), static_cast<ID3D11RenderTargetView*>(in.position->rtv),
            static_cast<ID3D11RenderTargetView*>(in.generic1->rtv), static_cast<ID3D11DepthStencilView*>(in.engine_dsv),
            in.output_width, in.output_height};
        const bool ok = upscale_pass.run(ctx, views);
        if (capture && capture->active) {
            take(ctx, "anomaly_seam_generic0", "SRStage=2: engine UNORM8 generic0 after the bilinear upscale to output size; input to every post pass",
                in.generic0->surface);
            capture->seam_taken = true;
        }
        return ok;
    }
    void set_viewport(uint32_t width, uint32_t height) noexcept override { set_full_viewport(context(), width, height); }
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
    void take(ID3D11DeviceContext* ctx, const char* name, const char* description, void* texture) noexcept {
        if (!dump_ready || !capture || !ctx || !texture) return;
        if (dump.texture_record(ctx, &capture->record, name, description, texture, RSF_DUMP_VIEW_RAW, 1.0f)) ++capture->images;
        else ++capture->failed;
    }
};

// ---- process state --------------------------------------------------------------------------------------------
std::mutex guard;
Host host;
SwapController controller{host};
Capture capture;
HookSet hooks;

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
    const bool scheduled = capture.remaining && world_frames >= capture.next;
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
    if (configured_options.stage != 2) make_plan(facts.device_width, facts.device_height, 1.0f, last_plan);
    if (configured_options.stage == 2) {
        // A device reset (vid_restart, resolution change) re-reads SRScale, so a new value applies without a restart.
        if (facts.generation != scale_generation) {
            if (scale_generation != ~0ull && !configured_options.ini.empty()) {
                const float scale = read_scale(configured_options.ini.c_str());
                if (scale != current_scale) {
                    char text[120];
                    std::snprintf(text, sizeof(text), "Anomaly SR stage 2: SRScale %.3f -> %.3f after a device reset.", current_scale, scale);
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
        facts.stage = 2;
        facts.device_size = image + device_width_rva;
        facts.base_zb = image + hw_base_zb_rva;
        facts.base_rt = image + hw_base_rt_rva;
        facts.ssao_viewport = image + ssao_viewport_rva;
        facts.ssao_viewport_guard = image + ssao_viewport_guard_rva;
    }
    return facts;
}

void __fastcall prepare_hook(void* self) {
    {
        std::lock_guard lock(guard);
        // A window still open here (never expected) is closed first, so Device.dwWidth reads the output size again.
        controller.close_stale();
        gpu_timer.begin(host.device(), host.context());
        const auto facts = gather(self);
        prepared_this_frame = true;
        controller.prepare(facts);
        if (!facts.scope) {
            ++world_frames;
            if (configured_options.capture) capture_arm(facts);
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
bool guards_match(std::byte* module, uint32_t image_size, const char** failed) noexcept {
    // Stage 2 writes the SSAO helper's static viewport: its code, which encodes both data addresses, must match.
    if (configured_options.stage == 2 && !bytes_match(module + ssao_set_viewport_rva, ssao_set_viewport_code)) {
        *failed = "set_viewport (r4_rendertarget_phase_ssao.cpp:3)"; return false;
    }
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
void bind_dump() noexcept try {
    const HMODULE runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    if (!runtime || !dump.bind(runtime)) { say("Anomaly SR stage capture refused: Runtime frame dump exports missing."); return; }
    wchar_t root[32768]{};
    std::wstring directory;
    const DWORD custom = GetEnvironmentVariableW(L"RSF_DUMP_DIR", root, 32768);
    if (custom && custom < 32768) directory = root;
    else {
        const DWORD size = GetEnvironmentVariableW(L"LOCALAPPDATA", root, 32768);
        if (!size || size >= 32768) { say("Anomaly SR stage capture refused: LOCALAPPDATA unavailable."); return; }
        directory = std::wstring(root) + L"\\ReScaleFrame\\Anomaly";
    }
    dump.configure(utf8(directory).c_str(), "Anomaly", dx11_hash, log_fn, log_user);
    dump_ready = true;
    capture.remaining = configured_options.capture_count;
    capture.next = configured_options.capture_start;
    host.capture = &capture;
} catch (...) { dump_ready = false; }
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

StageOptions read_options(const wchar_t* ini) noexcept try {
    StageOptions result{};
    if (!ini) return result;
    const auto stage = GetPrivateProfileIntW(L"Anomaly", L"SRStage", 0, ini);
    result.stage = stage == 1 || stage == 2 ? static_cast<uint32_t>(stage) : 0u;
    result.scale = read_scale(ini, &result.scale_defaulted);
    result.ini = ini;
    result.capture = GetPrivateProfileIntW(L"Anomaly", L"SRStageCapture", 0, ini) != 0;
    result.capture_start = GetPrivateProfileIntW(L"Anomaly", L"SRStageCaptureStart", 600, ini);
    result.capture_count = GetPrivateProfileIntW(L"Anomaly", L"SRStageCaptureCount", 3, ini);
    result.capture_interval = GetPrivateProfileIntW(L"Anomaly", L"SRStageCaptureInterval", 300, ini);
    if (result.capture_count > 16) result.capture_count = 16;
    if (!result.capture_interval) result.capture_interval = 1;
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
    controller.set_swap_enabled(configured_options.stage == 1 || configured_options.stage == 2);
    current_scale = clamp_scale(configured_options.scale);
    scale_generation = ~0ull;
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
    if (configured_options.stage == 1 || configured_options.stage == 2) geometry::geometry_set_rm_near_callback(&rm_near_callback);
    hooks_active = true;
    lock.unlock();
    if (configured_options.capture) bind_dump();
    char text[400];
    char scale[96];
    if (configured_options.stage == 2)
        std::snprintf(scale, sizeof(scale), "SRScale=%.3f%s, bilinear upscale at the seam", current_scale,
            configured_options.scale_defaulted ? " (default)" : "");
    else std::snprintf(scale, sizeof(scale), "scale 1.0");
    std::snprintf(text, sizeof(text),
        "Anomaly SR stage hooks installed: SRStage=%u SRStageCapture=%d (start=%u count=%u interval=%u) rmNear slot=%s; %s, no SR provider is called.",
        configured_options.stage, configured_options.capture ? 1 : 0, configured_options.capture_start, configured_options.capture_count,
        configured_options.capture_interval,
        configured_options.stage ? (geometry::geometry_hooks_active() ? "shared" : "unavailable (geometry hooks refused)") : "unused", scale);
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
    if (dump_ready) { capture_abandon(); dump.cancel(session); }
    host.pass.reset();
    host.upscale_pass.reset();
    if (disabled) hooks_active = false;
    return disabled;
}

void frame_end() noexcept {
    if (!hooks_active.load(std::memory_order_acquire)) return;
    std::lock_guard lock(guard);
    controller.frame_end();
    ++frames;
    if (!prepared_this_frame) ++frames_without_world;   // menu, loading, first frame after reset
    prepared_this_frame = false;
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
    struct { double world[2], seam; uint64_t samples[2], seam_samples, skipped; } timer{};
    {
        std::lock_guard lock(guard);
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
    char text[720];
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
        configured_options.stage == 2 ? plan.render_width : plan.output_width, configured_options.stage == 2 ? plan.render_height : plan.output_height,
        plan.output_width, plan.output_height, configured_options.stage == 2 ? double(scale) : 1.0, double(bytes) / (1024.0 * 1024.0),
        refusal ? refusal : "no");
    emit(user, text);
    reported = now; reported_frames = now_frames; reported_without_world = now_without;
    reported_world_ms[0] = timer.world[0]; reported_world_ms[1] = timer.world[1]; reported_seam_ms = timer.seam;
    reported_world_samples[0] = timer.samples[0]; reported_world_samples[1] = timer.samples[1]; reported_seam_samples = timer.seam_samples;
}
}
