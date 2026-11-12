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
#include <cstring>
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
    RoundTripPass pass;
    Capture* capture{};
    uint64_t last_drop_refs{};
    bool read(const void* address, void* output, std::size_t size) noexcept override { return guarded_copy(output, address, size); }
    bool write(void* address, const void* input, std::size_t size) noexcept override { return guarded_copy(address, input, size); }
    ID3D11Device* device() noexcept { ID3D11Device* value{}; return read_global(hw_device_rva, value) ? value : nullptr; }
    ID3D11DeviceContext* context() noexcept { ID3D11DeviceContext* value{}; return read_global(hw_context_rva, value) ? value : nullptr; }
    bool make_twins(const std::array<EngineTarget, slot_count>& engine, std::array<TargetFields, slot_count>& out) noexcept override {
        auto* d3d = device();
        if (!d3d) { say("Anomaly SR stage: HW.pDevice is null; twins not created."); return false; }
        if (!pass.ready() || pass.device() != d3d) {
            if (!pass.create(d3d, log_fn, log_user)) { say("Anomaly SR stage: round-trip shader could not be created."); return false; }
        }
        for (std::size_t i = 0; i < slot_count; ++i) {
            const auto error = make_twin(d3d, static_cast<ID3D11Resource*>(engine[i].fields.surface), engine[i].fields.uav != nullptr, twins[i]);
            if (error != TwinError::None) {
                char text[160];
                std::snprintf(text, sizeof(text), "Anomaly SR stage: twin %zu refused (%s).", i, twin_error_name(error));
                say(text);
                twins = {};
                return false;
            }
            out[i] = twins[i].fields;
        }
        char text[200];
        std::snprintf(text, sizeof(text), "Anomaly SR stage: FP16 twins created for generic0/generic_temp at %ux%u (uav=%d/%d).",
            out[0].width, out[0].height, out[0].uav ? 1 : 0, out[1].uav ? 1 : 0);
        say(text);
        return true;
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
        last_drop_refs = refs;
    }
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
std::atomic<bool> hooks_active{};
std::array<void*, 4> originals{};
uint64_t session{};
uint64_t world_frames{}, frames{}, frames_without_world{};
bool prepared_this_frame{};
SwapCounters reported{};
uint64_t reported_frames{}, reported_without_world{};
using Unary = void(__fastcall*)(void*);

void build_record(uint32_t width, uint32_t height) noexcept {
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
    r.render_width = r.output_width = width;
    r.render_height = r.output_height = height;
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
    build_record(facts.device_width, facts.device_height);
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
    return facts;
}

void __fastcall prepare_hook(void* self) {
    {
        std::lock_guard lock(guard);
        const auto facts = gather(self);
        prepared_this_frame = true;
        controller.prepare(facts);
        if (!facts.scope) {
            ++world_frames;
            if (configured_options.capture) capture_arm(facts);
        }
    }
    reinterpret_cast<Unary>(originals[0])(self);
}
void seam(void* self) noexcept {
    std::lock_guard lock(guard);
    if (controller.window_open()) { controller.seam(); return; }
    controller.seam();   // counted as a pass-through
    // Stock capture: the engine generic0 as the post chain receives it, once per frame at the first seam call.
    if (capture.active && !capture.seam_taken) {
        std::byte* target{};
        std::byte* crt{};
        void* surface{};
        if (read_global(render_implementation_rva + render_target_offset, target) && target == self &&
            guarded_copy(&crt, target + 0x3a8, sizeof(crt)) && crt && guarded_copy(&surface, crt + crt_surface_offset, sizeof(surface)))
            host.take(host.context(), "anomaly_seam_generic0",
                controller.refused() || configured_options.stage != 1
                    ? "Stock (no swap this frame): engine UNORM8 generic0 at the S-B seam; input to every post pass"
                    : "SRStage=1 frame without a window: engine UNORM8 generic0 at the S-B seam",
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
bool guards_match(std::byte* module, uint32_t image_size, const char** failed) noexcept {
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

StageOptions read_options(const wchar_t* ini) noexcept {
    StageOptions result{};
    if (!ini) return result;
    result.stage = GetPrivateProfileIntW(L"Anomaly", L"SRStage", 0, ini) == 1 ? 1u : 0u;
    result.capture = GetPrivateProfileIntW(L"Anomaly", L"SRStageCapture", 0, ini) != 0;
    result.capture_start = GetPrivateProfileIntW(L"Anomaly", L"SRStageCaptureStart", 600, ini);
    result.capture_count = GetPrivateProfileIntW(L"Anomaly", L"SRStageCaptureCount", 3, ini);
    result.capture_interval = GetPrivateProfileIntW(L"Anomaly", L"SRStageCaptureInterval", 300, ini);
    if (result.capture_count > 16) result.capture_count = 16;
    if (!result.capture_interval) result.capture_interval = 1;
    return result;
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
    controller.set_swap_enabled(configured_options.stage == 1);
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
    if (configured_options.stage == 1) geometry::geometry_set_rm_near_callback(&rm_near_callback);
    hooks_active = true;
    lock.unlock();
    if (configured_options.capture) bind_dump();
    char text[320];
    std::snprintf(text, sizeof(text),
        "Anomaly SR stage hooks installed: SRStage=%u SRStageCapture=%d (start=%u count=%u interval=%u) rmNear slot=%s; scale 1.0, no SR provider is called.",
        configured_options.stage, configured_options.capture ? 1 : 0, configured_options.capture_start, configured_options.capture_count,
        configured_options.capture_interval,
        configured_options.stage == 1 ? (geometry::geometry_hooks_active() ? "shared" : "unavailable (geometry hooks refused)") : "unused");
    say(text);
    return true;
} catch (...) { return false; }

bool quiesce() noexcept {
    if (!hooks.created()) return true;
    geometry::geometry_set_rm_near_callback(nullptr);
    const bool disabled = hooks.disable();
    std::lock_guard lock(guard);
    controller.shutdown();
    if (dump_ready) { capture_abandon(); dump.cancel(session); }
    host.pass.reset();
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
    uint64_t now_frames, now_without, twin_refs;
    Capture cap;
    const char* refusal;
    {
        std::lock_guard lock(guard);
        now = controller.counters();
        now_frames = frames; now_without = frames_without_world; twin_refs = host.last_drop_refs;
        cap = capture; refusal = controller.refusal();
    }
    const auto d = [&](uint64_t SwapCounters::*field) { return static_cast<unsigned long long>(now.*field - reported.*field); };
    const auto skip = [&](Skip s) { const auto i = static_cast<std::size_t>(s); return static_cast<unsigned long long>(now.skipped[i] - reported.skipped[i]); };
    const auto rest = [&](Restore r) { const auto i = static_cast<std::size_t>(r); return static_cast<unsigned long long>(now.restores[i] - reported.restores[i]); };
    char text[600];
    std::snprintf(text, sizeof(text),
        "Anomaly SR stage window frames=%llu no_world=%llu prepare=%llu eligible=%llu swapped=%llu skipped off=%llu scope=%llu msaa=%llu "
        "layout=%llu twins=%llu | seams=%llu passthrough=%llu round_trips=%llu rt_failed=%llu | restores seam=%llu combine_end=%llu "
        "stale_prepare=%llu frame_end=%llu shutdown=%llu rollback=%llu | rm_near_in_window=%llu foreign=%llu write_faults=%llu",
        static_cast<unsigned long long>(now_frames - reported_frames), static_cast<unsigned long long>(now_without - reported_without_world),
        d(&SwapCounters::prepares), d(&SwapCounters::eligible), d(&SwapCounters::swapped), skip(Skip::Off), skip(Skip::Scope),
        skip(Skip::Msaa), skip(Skip::Layout), skip(Skip::Twins), d(&SwapCounters::seams), d(&SwapCounters::seams_without_window),
        d(&SwapCounters::round_trips), d(&SwapCounters::round_trip_failures), rest(Restore::Seam), rest(Restore::CombineEnd),
        rest(Restore::StalePrepare), rest(Restore::FrameEnd), rest(Restore::Shutdown), rest(Restore::Rollback),
        d(&SwapCounters::rm_near_in_window), d(&SwapCounters::foreign_changes), d(&SwapCounters::write_faults));
    emit(user, text);
    uint64_t restores{};
    for (const auto value : now.restores) restores += value;
    std::snprintf(text, sizeof(text),
        "Anomaly SR stage totals stage=%u swapped=%llu restores=%llu round_trips=%llu twin_generation=%llu twin_drops=%llu twin_refs_at_drop=%llu "
        "captures=%llu images=%llu capture_failed=%llu refused=%s",
        configured_options.stage, static_cast<unsigned long long>(now.swapped), static_cast<unsigned long long>(restores),
        static_cast<unsigned long long>(now.round_trips), static_cast<unsigned long long>(now.twin_generations),
        static_cast<unsigned long long>(now.twin_drops), static_cast<unsigned long long>(twin_refs),
        static_cast<unsigned long long>(cap.taken), static_cast<unsigned long long>(cap.images), static_cast<unsigned long long>(cap.failed),
        refusal ? refusal : "no");
    emit(user, text);
    reported = now; reported_frames = now_frames; reported_without_world = now_without;
}
}
