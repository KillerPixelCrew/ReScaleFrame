// SPDX-License-Identifier: GPL-3.0-only
#include "sr_temporal.h"
#include "sr_plan.h"
#include <rescaleframe/sr_session.h>
#include <d3d11.h>
#include <cstdio>
#include <cstring>
#include <iterator>

namespace rsf::anomaly::sr {
static_assert(backend_dlss == RSF_SR_DLSS && backend_fsr3 == RSF_SR_FSR3 && backend_fsr4 == RSF_SR_FSR4 && backend_xess == RSF_SR_XESS);
static_assert(motion_validity_channel == 2, "the core motion target stores validity in Z");
namespace {
struct Exclusive {
    SRWLOCK& lock;
    explicit Exclusive(SRWLOCK& l) noexcept : lock(l) { AcquireSRWLockExclusive(&lock); }
    ~Exclusive() { ReleaseSRWLockExclusive(&lock); }
};
struct Shared {
    SRWLOCK& lock;
    explicit Shared(SRWLOCK& l) noexcept : lock(l) { AcquireSRWLockShared(&lock); }
    ~Shared() { ReleaseSRWLockShared(&lock); }
};
const char* backend_name(uint32_t backend) noexcept {
    switch (backend) {
    case RSF_SR_DLSS: return "DLSS";
    case RSF_SR_FSR2: return "FSR2";
    case RSF_SR_FSR3: return "FSR3";
    case RSF_SR_FSR4: return "FSR4";
    case RSF_SR_XESS: return "XeSS";
    default: return "none";
    }
}
const char* result_name(int32_t result) noexcept {
    switch (result) {
    case RSF_DLSS_PIPELINE_OK: return "ok";
    case RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT: return "invalid argument";
    case RSF_DLSS_PIPELINE_ERROR_ABI_MISMATCH: return "ABI mismatch";
    case RSF_DLSS_PIPELINE_ERROR_ALREADY_RUNNING: return "already running";
    case RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING: return "not running";
    case RSF_DLSS_PIPELINE_ERROR_STREAMLINE_FAILED: return "Streamline failed";
    case RSF_DLSS_PIPELINE_ERROR_NOT_SUPPORTED: return "not supported";
    case RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED: return "resource failed";
    case RSF_DLSS_PIPELINE_ERROR_MOTION_DECODE_FAILED: return "motion decode failed";
    case RSF_DLSS_PIPELINE_ERROR_FRAME_REFUSED: return "frame refused";
    case RSF_DLSS_PIPELINE_ERROR_EVALUATE_FAILED: return "evaluate failed";
    default: return "other";
    }
}
std::size_t result_index(int32_t result) noexcept { return result < 0 && result >= -10 ? std::size_t(-result) : 11u; }
}

const char* refusal_name(Refusal r) noexcept {
    static constexpr const char* names[] = {"not_running", "disabled", "prepare", "pass", "pipeline", "reinsert"};
    const auto i = static_cast<std::size_t>(r);
    return i < std::size(names) ? names[i] : "unknown";
}

ApiStatus bind_sr_api(rsf_get_d3d11_sr_api_fn query, rsf_d3d11_sr_api& out, const char** reason) noexcept {
    const char* unused = nullptr;
    if (!reason) reason = &unused;
    if (!query) { *reason = "the Runtime does not export rsf_get_d3d11_sr_api"; return ApiStatus::Missing; }
    rsf_d3d11_sr_api table{};
    table.struct_size = sizeof(table);
    table.abi_version = RSF_D3D11_SR_API_VERSION;
    const rsf_result result = query(&table);
    if (result == RSF_ERROR_ABI_MISMATCH) { *reason = "the Runtime's SR API version does not match"; return ApiStatus::Refused; }
    if (result != RSF_OK) { *reason = "the Runtime refused the SR API query"; return ApiStatus::Refused; }
    if (table.struct_size < sizeof(table) || table.abi_version != RSF_D3D11_SR_API_VERSION) {
        *reason = "the Runtime's SR API table is short or of another version"; return ApiStatus::Refused;
    }
    if (table.pipeline_abi_version != RSF_DLSS_PIPELINE_ABI_VERSION) {
        *reason = "the Runtime's SR pipeline structures are of another version"; return ApiStatus::Refused;
    }
    if (!table.start || !table.evaluate || !table.reset || !table.stop || !table.status || !table.select_backend ||
        !table.set_quality || !table.set_preset || !table.resize_output || !table.output_texture || !table.request_dump) {
        *reason = "the Runtime's SR API table has a missing entry"; return ApiStatus::Refused;
    }
    out = table;
    return ApiStatus::Bound;
}

void TemporalDriver::say(const char* text) const noexcept {
    if (log_) try { log_(log_user_, text); } catch (...) {}
}

void TemporalDriver::configure(const TemporalOptions& options, rsf_game_log_fn log, void* user) noexcept {
    options_ = options;
    log_ = log;
    log_user_ = user;
    Exclusive lock(lock_);
    requested_ = options.initial;
    have_request_ = true;
    status_.requested = options.initial;
}

bool TemporalDriver::bind_table(const rsf_d3d11_sr_api& table) noexcept {
    static const rsf_d3d11_sr_api* injected;
    injected = &table;
    const char* reason = nullptr;
    const auto status = bind_sr_api([](rsf_d3d11_sr_api* out) -> rsf_result {
        if (!out || out->struct_size < sizeof(*out)) return RSF_ERROR_INVALID_ARGUMENT;
        *out = *injected;
        return RSF_OK;
    }, api_, &reason);
    bound_ = status == ApiStatus::Bound;
    if (!bound_) { std::string text = "Anomaly SR stage 5: "; text += reason; say(text.c_str()); }
    Exclusive lock(lock_);
    status_.bound = bound_;
    if (!bound_) status_.reason = reason ? reason : "SR API refused";
    return bound_;
}

bool TemporalDriver::bind(HMODULE runtime) noexcept {
    const auto query = runtime ? reinterpret_cast<rsf_get_d3d11_sr_api_fn>(
        reinterpret_cast<void*>(GetProcAddress(runtime, RSF_D3D11_SR_API_ENTRY_POINT))) : nullptr;
    const char* reason = nullptr;
    bound_ = bind_sr_api(query, api_, &reason) == ApiStatus::Bound;
    LARGE_INTEGER qpc{};
    QueryPerformanceCounter(&qpc);
    session_ = static_cast<uint64_t>(qpc.QuadPart) | 1u;
    char text[256];
    if (bound_) std::snprintf(text, sizeof(text), "Anomaly SR stage %u: Runtime SR API bound (API %u, pipeline ABI %u).",
        options_.stage, api_.abi_version, api_.pipeline_abi_version);
    else std::snprintf(text, sizeof(text), "Anomaly SR stage %u: %s; SR stays on the bilinear seam pass.", options_.stage,
        reason ? reason : "SR API refused");
    say(text);
    Exclusive lock(lock_);
    status_.bound = bound_;
    if (!bound_) status_.reason = reason ? reason : "SR API refused";
    return bound_;
}

void TemporalDriver::request(const SrSettings& wanted) noexcept {
    Exclusive lock(lock_);
    requested_ = sanitize(wanted, requested_);
    have_request_ = true;
    status_.requested = requested_;
}

bool TemporalDriver::enabled() const noexcept {
    Shared lock(lock_);
    return requested_.enabled;
}

uint32_t TemporalDriver::output_format() const noexcept {
    if (!running_ || !bound_) return 0;
    auto* texture = static_cast<ID3D11Texture2D*>(api_.output_texture());
    if (!texture) return 0;
    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    return desc.Format;
}

bool TemporalDriver::planned_render(uint32_t ow, uint32_t oh, uint32_t& rw, uint32_t& rh) const noexcept {
    Shared lock(lock_);
    const auto& p = status_.pipeline;
    if (!running_ || !p.running || p.output_width != ow || p.output_height != oh || !p.render_width || !p.render_height) return false;
    ScalePlan plan{};
    if (!make_plan_for_render(ow, oh, p.render_width, p.render_height, plan)) return false;
    rw = plan.render_width;
    rh = plan.render_height;
    return true;
}

void TemporalDriver::refresh() noexcept {
    rsf_dlss_pipeline_status pipeline{};
    pipeline.struct_size = sizeof(pipeline);
    const bool have = bound_ && api_.status(&pipeline) == RSF_DLSS_PIPELINE_OK;
    Exclusive lock(lock_);
    if (have) status_.pipeline = pipeline;
    else { status_.pipeline = {}; status_.pipeline.struct_size = sizeof(pipeline); }
    status_.running = running_;
    status_.applied = applied_;
}

void TemporalDriver::start(ID3D11Device* device, uint32_t ow, uint32_t oh) noexcept {
    SrSettings wanted;
    { Shared lock(lock_); wanted = requested_; }
    if (failed_device_ == device && failed_settings_ == wanted) return;   // not retried until something changes
    rsf_dlss_pipeline_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
    setup.streamline_directory_utf8 = options_.streamline.c_str();
    setup.require_signature = options_.require_signature ? 1u : 0u;
    setup.output_width = ow;
    setup.output_height = oh;
    setup.quality = static_cast<rsf_dlss_quality>(wanted.quality);
    setup.motion = motion_decode_params();
    setup.log = log_;
    setup.log_user = log_user_;
    setup.engine = RSF_DLSS_ENGINE_CUSTOM;
    setup.engine_version_utf8 = engine_version;
    setup.project_id_utf8 = project_id;
    setup.fsr2_directory_utf8 = "";
    setup.fsr3_directory_utf8 = options_.fidelityfx.c_str();
    setup.fsr4_directory_utf8 = options_.fidelityfx.c_str();
    setup.xess_directory_utf8 = options_.xess.c_str();
    setup.view_space_to_meters = view_space_to_meters;
    setup.standard_depth = 1;
    char text[400];
    std::snprintf(text, sizeof(text), "Anomaly SR stage 5: starting the shared SR pipeline at %ux%u (backend %s, quality %u, preset %u; "
        "Streamline %s).", ow, oh, backend_name(wanted.backend), wanted.quality, wanted.preset, options_.streamline.c_str());
    say(text);
    const auto started = api_.start(device, &setup);
    { Exclusive lock(lock_); status_.start_result = started; }
    if (started != RSF_DLSS_PIPELINE_OK) {
        ++counters_.start_failures;
        failed_device_ = device;
        failed_settings_ = wanted;
        std::snprintf(text, sizeof(text), "Anomaly SR stage 5: the SR pipeline did not start (%d, %s)%s; the seam stays bilinear.",
            started, result_name(started), started == RSF_DLSS_PIPELINE_ERROR_NOT_SUPPORTED
                ? ". The shared D3D11 pipeline starts only where DLSS is supported; FSR and XeSS are reached through it" : "");
        say(text);
        Exclusive lock(lock_);
        status_.reason = started == RSF_DLSS_PIPELINE_ERROR_NOT_SUPPORTED ? "The SR pipeline needs DLSS support on this GPU"
            : "The SR pipeline did not start";
        return;
    }
    ++counters_.starts;
    running_ = true;
    device_ = device;
    failed_device_ = nullptr;
    first_ = true;
    applied_ = SrSettings{wanted.enabled, backend_dlss, wanted.quality, 0};
    have_refused_ = false;
    apply(wanted);
}

void TemporalDriver::apply(const SrSettings& wanted) noexcept {
    char text[200];
    if (wanted.backend != applied_.backend) {
        ++counters_.selects;
        const auto result = api_.select_backend(wanted.backend, nullptr, nullptr);
        { Exclusive lock(lock_); status_.select_result = result; }
        if (result == RSF_DLSS_PIPELINE_OK) { applied_.backend = wanted.backend; settings_changed_ = true; }
        else {
            ++counters_.select_failures;
            std::snprintf(text, sizeof(text), "Anomaly SR stage 5: backend %s refused (%d, %s); keeping %s.", backend_name(wanted.backend),
                result, result_name(result), backend_name(applied_.backend));
            say(text);
        }
    }
    if (wanted.quality != applied_.quality) {
        ++counters_.qualities;
        uint32_t rw{}, rh{};
        const auto result = api_.set_quality(wanted.quality, &rw, &rh);
        { Exclusive lock(lock_); status_.quality_result = result; }
        if (result == RSF_DLSS_PIPELINE_OK) { applied_.quality = wanted.quality; settings_changed_ = true; }
        else {
            ++counters_.quality_failures;
            std::snprintf(text, sizeof(text), "Anomaly SR stage 5: quality %u refused (%d, %s); keeping %u.", wanted.quality, result,
                result_name(result), applied_.quality);
            say(text);
        }
    }
    if (wanted.preset != applied_.preset) {
        // The user's DLSSPreset, forwarded as chosen. The adapter never selects a preset itself.
        if (api_.set_preset(wanted.preset) == RSF_DLSS_PIPELINE_OK) { ++counters_.presets; applied_.preset = wanted.preset; settings_changed_ = true; }
    }
    if (wanted.enabled != applied_.enabled) { applied_.enabled = wanted.enabled; settings_changed_ = true; }
    // A backend or quality this GPU refused is not asked for again every frame; a new request retries it.
    have_refused_ = !(wanted == applied_);
    refused_request_ = wanted;
}

void TemporalDriver::boundary(ID3D11Device* device, uint32_t ow, uint32_t oh) noexcept {
    if (options_.stage != 5 || !bound_) return;
    if (running_ && device != device_) {
        // vid_restart with a new device: the pipeline holds the old one; restart on the new device.
        say("Anomaly SR stage 5: the engine device changed; restarting the SR pipeline on the new device.");
        stop();
        device_changed_ = true;
    }
    SrSettings wanted;
    { Shared lock(lock_); wanted = requested_; }
    if (!running_) {
        if (device && ow && oh && wanted.enabled) start(device, ow, oh);
    } else if (!(wanted == applied_) && !(have_refused_ && wanted == refused_request_)) apply(wanted);
    if (running_) {
        rsf_dlss_pipeline_status pipeline{};
        pipeline.struct_size = sizeof(pipeline);
        if (api_.status(&pipeline) == RSF_DLSS_PIPELINE_OK && (pipeline.output_width != ow || pipeline.output_height != oh) && ow && oh) {
            // Resolution change: plan the next frame's render size for the new output now, not one frame late.
            ++counters_.resizes;
            if (api_.resize_output(ow, oh, nullptr, nullptr) != RSF_DLSS_PIPELINE_OK) ++counters_.resize_failures;
        }
    }
    refresh();
}

bool TemporalDriver::evaluate(ID3D11DeviceContext* context, PassInputs& in, const ResetFacts& facts_in) noexcept {
    if (!running_ || !bound_) { count(Refusal::NotRunning); return false; }
    ResetFacts facts = facts_in;
    facts.first = facts.first || first_;
    facts.device = facts.device || device_changed_;
    facts.settings = facts.settings || settings_changed_;
    const auto reason = reset_reason(facts);
    in.reset = reason != ResetReason::None;
    in.session = session_;
    rsf_game_render_pass pass;
    const auto error = build_pass(in, pass);
    if (error != PassError::None) {
        ++counters_.pass_errors[static_cast<std::size_t>(error)];
        count(Refusal::Pass);
        Exclusive lock(lock_);
        status_.last_evaluated = false;
        status_.reason = std::string("SR input refused: ") + pass_error_name(error);
        return false;
    }
    const auto result = api_.evaluate(context, &pass);
    rsf_dlss_pipeline_status pipeline{};
    pipeline.struct_size = sizeof(pipeline);
    const bool have = api_.status(&pipeline) == RSF_DLSS_PIPELINE_OK;
    Exclusive lock(lock_);
    if (have) status_.pipeline = pipeline;
    status_.jitter[0] = in.jitter.current_x;
    status_.jitter[1] = in.jitter.current_y;
    status_.motion_present = in.motion != nullptr && !in.motion_missing;
    status_.exposure_present = in.exposure != nullptr;
    if (result != RSF_DLSS_PIPELINE_OK) {
        ++counters_.results[result_index(result)];
        ++counters_.refused[static_cast<std::size_t>(Refusal::Pipeline)];
        status_.last_evaluated = false;
        char text[96];
        std::snprintf(text, sizeof(text), "SR pipeline refused the frame (%d, %s)", result, result_name(result));
        status_.reason = text;
        return false;
    }
    if (in.reset) ++counters_.resets[static_cast<std::size_t>(reason)];
    first_ = device_changed_ = settings_changed_ = false;
    const uint32_t backend = have ? pipeline.backend : applied_.backend;
    ++counters_.evaluated[backend < counters_.evaluated.size() ? backend : 0];
    status_.last_evaluated = true;
    status_.last_backend = backend;
    status_.last_render_width = in.render_width;
    status_.last_render_height = in.render_height;
    status_.last_output_width = in.output_width;
    status_.last_output_height = in.output_height;
    status_.reason.clear();
    return true;
}

bool TemporalDriver::request_dump(const std::string& prefix) noexcept {
    return running_ && bound_ && !prefix.empty() && api_.request_dump(prefix.c_str()) == RSF_DLSS_PIPELINE_OK;
}

void TemporalDriver::count(Refusal r) noexcept {
    Exclusive lock(lock_);
    ++counters_.refused[static_cast<std::size_t>(r)];
    if (r != Refusal::Pipeline && r != Refusal::Pass) {
        status_.last_evaluated = false;
        if (r == Refusal::Reinsert) status_.reason = "SR output pass failed; bilinear fallback drawn";
        else if (r == Refusal::Prepare) status_.reason = "SR input preparation failed";
        else if (r == Refusal::Disabled) status_.reason = "SR disabled by setting";
        else if (status_.reason.empty()) status_.reason = "SR pipeline not running";
    }
}
void TemporalDriver::count_bilinear() noexcept {
    Exclusive lock(lock_);
    ++counters_.bilinear;
}
void TemporalDriver::count_prepared(bool motion_missing) noexcept {
    Exclusive lock(lock_);
    ++counters_.prepared;
    if (motion_missing) ++counters_.motion_missing;
}

void TemporalDriver::stop() noexcept {
    if (!running_) return;
    if (bound_) api_.stop();
    running_ = false;
    device_ = nullptr;
    ++counters_.stops;
    {
        Exclusive lock(lock_);
        status_.last_evaluated = false;
        status_.running = false;
        status_.reason = "SR pipeline stopped";
    }
    refresh();
}

TemporalStatus TemporalDriver::status() const noexcept {
    Shared lock(lock_);
    return status_;
}
TemporalCounters TemporalDriver::counters() const noexcept {
    Shared lock(lock_);
    return counters_;
}

bool TemporalDriver::overlay_report(rsf_overlay_sr_report& r) const noexcept {
    Shared lock(lock_);
    const auto& s = status_;
    r = {};
    r.struct_size = sizeof(r);
    const bool active = s.last_evaluated;
    r.effective_backend = active ? s.last_backend : 0u;
    r.effective_quality = active ? s.applied.quality : 0u;
    r.render_width = s.last_render_width;
    r.render_height = s.last_render_height;
    r.output_width = s.last_output_width;
    r.output_height = s.last_output_height;
    uint64_t evaluated = 0, refused = 0;
    for (const auto v : counters_.evaluated) evaluated += v;
    for (const auto v : counters_.refused) refused += v;
    r.frames_evaluated = static_cast<uint32_t>(evaluated);
    r.frames_refused = static_cast<uint32_t>(refused);
    r.last_result = s.pipeline.last_result;
    if (active) {
        r.inputs = RSF_OVERLAY_SR_INPUT_COLOR | RSF_OVERLAY_SR_INPUT_DEPTH | RSF_OVERLAY_SR_INPUT_JITTER |
            (s.motion_present ? RSF_OVERLAY_SR_INPUT_MOTION : 0u) | (s.exposure_present ? RSF_OVERLAY_SR_INPUT_EXPOSURE : 0u);
        r.jitter_pixels[0] = s.jitter[0];
        r.jitter_pixels[1] = s.jitter[1];
    }
    r.quality_locked = 0;
    std::string reason;
    if (!active) reason = s.reason.empty() ? std::string(s.requested.enabled ? "SR not evaluated yet" : "SR disabled by setting") : s.reason;
    else if (s.requested.backend != s.applied.backend) reason = std::string(backend_name(s.requested.backend)) + " refused on this GPU";
    else if (s.requested.quality != s.applied.quality) reason = "Requested quality refused";
    std::snprintf(r.reason, sizeof(r.reason), "%s", reason.c_str());
    return true;
}

void TemporalDriver::report(void (*emit)(void*, const char*), void* user) noexcept {
    if (!emit) return;
    TemporalCounters now;
    TemporalStatus s;
    { Shared lock(lock_); now = counters_; s = status_; }
    const auto d = [&](uint64_t a, uint64_t b) { return static_cast<unsigned long long>(a - b); };
    const auto& o = reported_;
    char text[1400];
    std::snprintf(text, sizeof(text),
        "Anomaly SR stage %u evaluated dlss=%llu fsr3=%llu fsr4=%llu xess=%llu | refused not_running=%llu disabled=%llu prepare=%llu "
        "pass=%llu (sizes=%llu camera=%llu projection=%llu jitter=%llu resources=%llu) pipeline=%llu (invalid=%llu abi=%llu "
        "not_running=%llu streamline=%llu not_supported=%llu resource=%llu decode=%llu frame=%llu evaluate=%llu other=%llu) "
        "reinsert=%llu | resets first=%llu device=%llu settings=%llu sequence=%llu cut=%llu | prepared=%llu bilinear=%llu "
        "motion_missing=%llu",
        options_.stage, d(now.evaluated[RSF_SR_DLSS], o.evaluated[RSF_SR_DLSS]), d(now.evaluated[RSF_SR_FSR3], o.evaluated[RSF_SR_FSR3]),
        d(now.evaluated[RSF_SR_FSR4], o.evaluated[RSF_SR_FSR4]), d(now.evaluated[RSF_SR_XESS], o.evaluated[RSF_SR_XESS]),
        d(now.refused[0], o.refused[0]), d(now.refused[1], o.refused[1]), d(now.refused[2], o.refused[2]), d(now.refused[3], o.refused[3]),
        d(now.pass_errors[1], o.pass_errors[1]), d(now.pass_errors[2], o.pass_errors[2]), d(now.pass_errors[3], o.pass_errors[3]),
        d(now.pass_errors[4], o.pass_errors[4]), d(now.pass_errors[5], o.pass_errors[5]), d(now.refused[4], o.refused[4]),
        d(now.results[1], o.results[1]), d(now.results[2], o.results[2]), d(now.results[4], o.results[4]), d(now.results[5], o.results[5]),
        d(now.results[6], o.results[6]), d(now.results[7], o.results[7]), d(now.results[8], o.results[8]), d(now.results[9], o.results[9]),
        d(now.results[10], o.results[10]), d(now.results[11] + now.results[3], o.results[11] + o.results[3]), d(now.refused[5], o.refused[5]),
        d(now.resets[1], o.resets[1]), d(now.resets[5], o.resets[5]), d(now.resets[4], o.resets[4]), d(now.resets[2], o.resets[2]),
        d(now.resets[3], o.resets[3]), d(now.prepared, o.prepared), d(now.bilinear, o.bilinear), d(now.motion_missing, o.motion_missing));
    emit(user, text);
    if (options_.stage == 5) {
        const auto& p = s.pipeline;
        std::snprintf(text, sizeof(text),
            "Anomaly SR stage 5 pipeline bound=%d running=%d backend=%s requested=%s switch=%d quality=%u/%u preset=%u render=%ux%u "
            "output=%ux%u frames_evaluated=%llu frames_refused=%llu last=%d (%s) | starts=%llu start_failed=%llu (last %d) stops=%llu "
            "selects=%llu/%llu qualities=%llu/%llu presets=%llu resizes=%llu/%llu | SR=%u exposure=%s first_person_zero=%d | %s",
            s.bound ? 1 : 0, p.running ? 1 : 0, backend_name(p.backend), backend_name(s.requested.backend), p.last_switch_result,
            s.applied.quality, s.requested.quality, s.applied.preset, p.render_width, p.render_height, p.output_width, p.output_height,
            static_cast<unsigned long long>(p.frames_evaluated), static_cast<unsigned long long>(p.frames_refused), p.last_result,
            result_name(p.last_result), static_cast<unsigned long long>(now.starts), static_cast<unsigned long long>(now.start_failures),
            s.start_result, static_cast<unsigned long long>(now.stops), static_cast<unsigned long long>(now.selects - now.select_failures),
            static_cast<unsigned long long>(now.selects), static_cast<unsigned long long>(now.qualities - now.quality_failures),
            static_cast<unsigned long long>(now.qualities), static_cast<unsigned long long>(now.presets),
            static_cast<unsigned long long>(now.resizes - now.resize_failures), static_cast<unsigned long long>(now.resizes),
            s.requested.enabled ? 1u : 0u, options_.exposure_auto ? "auto" : "constant(1.0)", options_.first_person_zero ? 1 : 0,
            s.reason.empty() ? "ok" : s.reason.c_str());
        emit(user, text);
    }
    reported_ = now;
}
}
