// SPDX-License-Identifier: GPL-3.0-only
// The Runtime's shared overlay, published to DLL-loaded D3D11 adapters. It drives the existing
// D3D11 overlay host (panel, renderer, window/cursor input) on the chain the adapter presents and
// forwards frame-generation choices to the shared presentation owner. All policy is adapter data.
#include <rescaleframe/overlay_api.h>
#include <rescaleframe/overlay_fg_stats.h>
#include <rescaleframe/overlay_input.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/d3d11_present_bridge.h>
#include <rescaleframe/sr_session.h>
#include <rescaleframe/dlss_preset.h>
#include "../../../loader/proxy/src/overlay_host.h"
#include "render_settings_store.h"
#include <windows.h>
#include <dxgi.h>
#include <atomic>
#include <cstdarg>
#include <cstdio>
#include <mutex>
#include <string>

namespace {
constexpr uint32_t kKnownFg = (1u << RSF_FG_BACKEND_DLSS) | (1u << RSF_FG_BACKEND_FSR3) | (1u << RSF_FG_BACKEND_FSR4) |
    (1u << RSF_FG_BACKEND_XESS) | (1u << RSF_FG_BACKEND_AUTO);
constexpr uint32_t kKnownSr = (1u << RSF_SR_DLSS) | (1u << RSF_SR_FSR2) | (1u << RSF_SR_FSR3) | (1u << RSF_SR_FSR4) |
    (1u << RSF_SR_XESS) | (1u << RSF_SR_SPATIAL);
constexpr uint32_t kQualities = 0x1fu;
constexpr uint32_t kLatencies = (1u << RSF_REFLEX_OFF) | (1u << RSF_REFLEX_ON) | (1u << RSF_REFLEX_BOOST);

bool has(uint32_t mask, uint32_t id) { return id < 32 && (mask & (1u << id)) != 0; }

const char* sr_name(uint32_t backend)
{
    switch (backend) {
    case RSF_SR_DLSS: return "DLSS";
    case RSF_SR_FSR2: return "FSR2";
    case RSF_SR_FSR3: return "FSR3";
    case RSF_SR_FSR4: return "FSR4";
    case RSF_SR_XESS: return "XeSS";
    case RSF_SR_SPATIAL: return "Spatial";
    default: return "None";
    }
}

struct Service {
    std::mutex guard;
    std::once_flag claimed;
    std::atomic<bool> configured{};
    std::atomic<bool> started{};
    bool start_failed{};
    std::wstring ini, section;
    std::string identity;
    rsf_render_settings defaults{}, settings{};
    uint32_t sr_choices{}, quality_choices{}, fg_choices{}, latency_choices{}, max_generated{}, preset_available{};
    rsf_game_log_fn logger{};
    int (*accept)(void*, void*){};
    int (*report)(void*, rsf_overlay_sr_report*){};
    void (*changed)(void*, const rsf_render_settings*){};
    void* user{};
    // Sampled by present(); status() reads them.
    rsf_overlay_sr_report sr{};
    bool have_sr{}, sampled{};
    rsf_native_fg_status fg{};
    uint32_t fg_backend{}, fg_owner{}, bridge_choices{};
    int32_t switch_result{}, request_result{}, apply_result{};
    uint32_t chain_width{}, chain_height{};
    uint64_t presents{}, presents_drawn{};
    // Every settings request, from the panel or the API. Counted even when the log line is capped.
    uint32_t apply_attempts{}, apply_refused{};
    int32_t last_apply_status{};
    ULONGLONG rate_tick{};
    uint64_t rate_base{}, rate_output{};
    uint32_t rate_backend{};
    float base_fps{}, output_fps{};
    bool output_fps_valid{};
    uint32_t refresh_mhz{};
    ULONGLONG refresh_tick{};

    void say(const char* format, ...)
    {
        if (!logger) return;
        char message[256];
        va_list arguments; va_start(arguments, format);
        const int written = std::vsnprintf(message, sizeof(message), format, arguments);
        va_end(arguments);
        if (written >= 0) logger(user, message);
    }
    // No SR backend choice means the adapter has no super resolution: it can only be Off, and the SR
    // backend and quality stay at the adapter's defaults.
    bool sr_unavailable() const { return sr_choices == 0; }
    bool sr_valid(const rsf_render_settings& value) const
    {
        if (sr_unavailable())
            return !value.sr_enabled && value.sr_backend == defaults.sr_backend && value.sr_quality == defaults.sr_quality;
        return has(sr_choices, value.sr_backend) && has(quality_choices, value.sr_quality);
    }
    bool valid(const rsf_render_settings& value) const
    {
        return value.sr_enabled <= 1 && value.fg_enabled <= 1 && value.status_overlay <= 1 && sr_valid(value) &&
            has(latency_choices, value.latency) && value.generated_frames && value.generated_frames <= max_generated &&
            rsf_dlss_preset_valid(value.dlss_preset) && (!value.fg_enabled || fg_choices) &&
            (value.fg_backend == RSF_FG_BACKEND_AUTO || has(fg_choices, value.fg_backend));
    }
    // A saved value this adapter cannot apply falls back to its default, field by field.
    rsf_render_settings sanitize(rsf_render_settings value) const
    {
        if (value.sr_enabled > 1) value.sr_enabled = defaults.sr_enabled;
        if (value.fg_enabled > 1 || (value.fg_enabled && !fg_choices)) value.fg_enabled = defaults.fg_enabled;
        if (value.status_overlay > 1) value.status_overlay = defaults.status_overlay;
        if (sr_unavailable()) {
            value.sr_enabled = defaults.sr_enabled; value.sr_backend = defaults.sr_backend; value.sr_quality = defaults.sr_quality;
        } else {
            if (!has(sr_choices, value.sr_backend)) value.sr_backend = defaults.sr_backend;
            if (!has(quality_choices, value.sr_quality)) value.sr_quality = defaults.sr_quality;
        }
        if (!has(latency_choices, value.latency)) value.latency = defaults.latency;
        if (!value.generated_frames || value.generated_frames > max_generated) value.generated_frames = defaults.generated_frames;
        if (!rsf_dlss_preset_valid(value.dlss_preset)) value.dlss_preset = defaults.dlss_preset;
        if (value.fg_backend != RSF_FG_BACKEND_AUTO && !has(fg_choices, value.fg_backend)) value.fg_backend = defaults.fg_backend;
        return value;
    }
};
Service& service() { static auto* value = new Service; return *value; }

// The shared presentation owner takes generation requests; a provider change lands at a
// drained Present. Never called with the service lock held.
int32_t forward_fg(const rsf_render_settings& value)
{
    rsf_native_fg_status current{}; current.struct_size = sizeof(current);
    rsf_native_fg_status_get(&current);
    rsf_native_fg_options options = current.requested;
    options.struct_size = sizeof(options);
    options.mode = value.fg_enabled ? RSF_FG_FIXED : RSF_FG_OFF;
    options.generated_frames = value.generated_frames;
    options.reflex_mode = value.latency;
    options.frame_limit_us = value.frame_limit_us;
    rsf_native_fg_options_set(&options);
    if (!value.fg_enabled || value.fg_backend == RSF_FG_BACKEND_AUTO || !rsf_d3d11_present_has_owner() ||
        rsf_d3d11_present_backend() == value.fg_backend) return RSF_BACKEND_OK;
    return rsf_d3d11_present_request(value.fg_backend);
}

int32_t apply(Service& self, const rsf_render_settings& request)
{
    if (request.struct_size < sizeof(request)) return RSF_ERROR_INVALID_ARGUMENT;
    if (request.abi_version != RSF_RENDER_FRONTEND_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    if (!self.configured.load(std::memory_order_acquire)) return RSF_ERROR_NOT_READY;
    if (!self.valid(request)) return RSF_ERROR_INVALID_ARGUMENT;
    rsf_render_settings applied = request;
    applied.struct_size = sizeof(applied);
    {
        std::lock_guard lock(self.guard);
        const auto written = rsf::render_settings::write(self.ini, self.section, applied);
        if (written != RSF_OK) return written;
        self.settings = applied;
    }
    const auto requested = forward_fg(applied);
    {
        std::lock_guard lock(self.guard); self.request_result = requested;
    }
    if (requested != RSF_BACKEND_OK) self.say("RSF overlay: FG provider %u request result=%d.", applied.fg_backend, requested);
    if (self.changed) self.changed(self.user, &applied);
    return RSF_OK;
}

// One line per request and its outcome, so a run can show that a panel click reached the service
// and what became of it. Bounded: the counters keep counting after the lines stop.
constexpr uint32_t kApplyLogLimit = 200;
int32_t apply_logged(Service& self, const rsf_render_settings& request, const char* origin)
{
    const int32_t result = apply(self, request);
    uint32_t attempt;
    {
        std::lock_guard lock(self.guard);
        attempt = ++self.apply_attempts;
        if (result != RSF_OK) ++self.apply_refused;
        self.last_apply_status = result;
    }
    if (attempt <= kApplyLogLimit)
        self.say("RSF overlay: settings_apply #%u from %s: SR=%u SRBackend=%u SRQuality=%u FG=%u FGBackend=%u "
            "Generated=%u Latency=%u StatusOverlay=%u -> %s (%d).", attempt, origin, request.sr_enabled,
            request.sr_backend, request.sr_quality, request.fg_enabled, request.fg_backend,
            request.generated_frames, request.latency, request.status_overlay,
            result == RSF_OK ? "applied and saved" : "refused", result);
    return result;
}

// Shared by the panel's refusal line and status. Requires the service lock.
const char* describe(const Service& self, bool idle_text)
{
    const auto& value = self.settings;
    if (!self.sampled) return "Waiting for the first Present";
    if (value.sr_enabled && !self.sr.effective_backend) {
        if (self.have_sr && self.sr.reason[0]) return self.sr.reason;
        return self.have_sr ? "SR requested; the adapter reports no SR output" : "SR is not connected by the adapter";
    }    if (value.fg_enabled) {
        if (!self.fg_owner) return "FG presentation owner unavailable";
        if (self.request_result) return "FG provider request refused";
        if (self.switch_result) return "FG provider switch failed; previous provider retained";
        if (self.fg.last_result) return "FG backend error";
        if (self.fg.reason == RSF_NATIVE_FG_REASON_RESET) return "Resetting temporal history";
        if (self.fg.reason == RSF_NATIVE_FG_REASON_SCENE_UNSUPPORTED) return "Scene is not eligible";
        if (self.fg.reason == RSF_NATIVE_FG_REASON_HUDLESS_MISSING) return "HUD-less scene missing";
        if (self.fg.reason) return "Frame inputs or submission not matched";
        if (!self.fg.vendor.active) return "FG requested; the provider reports inactive";
    }
    if (self.sr_unavailable()) return "Super resolution is not available in this adapter";
    // A note from the adapter while SR is running, for example a backend that is still being built in the
    // background while the previous one keeps evaluating. Status only: it is not a refusal.
    if (idle_text && value.sr_enabled && self.have_sr && self.sr.effective_backend && self.sr.reason[0]) return self.sr.reason;
    if (!idle_text) return nullptr;
    return value.fg_enabled ? "Active" : "FG disabled by setting";
}

uint32_t display_refresh(Service& self, HWND window)
{
    const auto now = GetTickCount64();
    if (window && (!self.refresh_tick || now - self.refresh_tick >= 1000)) {
        self.refresh_tick = now ? now : 1;
        MONITORINFOEXW info{}; info.cbSize = sizeof(info);
        DEVMODEW mode{}; mode.dmSize = sizeof(mode);
        if (GetMonitorInfoW(MonitorFromWindow(window, MONITOR_DEFAULTTOPRIMARY), &info) &&
            EnumDisplaySettingsW(info.szDevice, ENUM_CURRENT_SETTINGS, &mode) && mode.dmDisplayFrequency > 1)
            self.refresh_mhz = mode.dmDisplayFrequency * 1000u;
    }
    return self.refresh_mhz;
}

// Present-owner thread: provider/session are stable until this Present's drained boundary.
void sample(Service& self, IDXGISwapChain* chain, const DXGI_SWAP_CHAIN_DESC& desc)
{
    rsf_overlay_sr_report report{}; report.struct_size = sizeof(report);
    const bool have_report = self.report && self.report(self.user, &report) != 0;
    report.reason[sizeof(report.reason) - 1] = '\0';
    rsf_native_fg_status fg{}; fg.struct_size = sizeof(fg);
    rsf_native_fg_status_get(&fg);
    const uint32_t owner = rsf_d3d11_present_has_owner() ? 1u : 0u;
    if (owner) {
        const auto* provider = rsf_d3d11_present_provider();
        void* session = rsf_d3d11_present_session();
        rsf_fg_status fresh{}; fresh.struct_size = sizeof(fresh);
        if (provider && session && provider->status && provider->status(session, &fresh) == RSF_BACKEND_OK) fg.vendor = fresh;
    }
    const uint32_t backend = owner ? rsf_d3d11_present_backend() : 0u;
    const uint32_t choices = rsf_d3d11_present_backend_choices();
    const int32_t switched = rsf_d3d11_present_switch_result();
    (void)chain;
    std::lock_guard lock(self.guard);
    self.sr = have_report ? report : rsf_overlay_sr_report{sizeof(rsf_overlay_sr_report)};
    self.have_sr = have_report; self.fg = fg; self.fg_owner = owner; self.fg_backend = backend;
    self.bridge_choices = choices; self.switch_result = switched;
    self.chain_width = desc.BufferDesc.Width; self.chain_height = desc.BufferDesc.Height;
    ++self.presents; self.sampled = true;
    const auto now = GetTickCount64();
    const bool counted = fg.available && (fg.vendor.valid_statistics & RSF_FG_STAT_TOTAL_PRESENTED);
    if (!self.rate_tick || self.rate_backend != backend || fg.vendor.total_presented < self.rate_output) {
        self.rate_tick = now; self.rate_base = self.presents; self.rate_output = fg.vendor.total_presented;
        self.rate_backend = backend; self.base_fps = self.output_fps = 0; self.output_fps_valid = false;
    } else if (now - self.rate_tick >= 500) {
        const float seconds = static_cast<float>(now - self.rate_tick) / 1000;
        self.base_fps = static_cast<float>(self.presents - self.rate_base) / seconds;
        self.output_fps = counted ? static_cast<float>(fg.vendor.total_presented - self.rate_output) / seconds : 0;
        self.output_fps_valid = counted;
        self.rate_tick = now; self.rate_base = self.presents; self.rate_output = fg.vendor.total_presented;
    }
}

rsf_render_settings with_intent(rsf_render_settings value, const rsf_overlay_intent& intent, bool& changed)
{
    if (intent.backend_changed) { value.sr_backend = intent.backend; changed = true; }
    if (intent.enabled_changed) { value.sr_enabled = intent.enabled != 0; changed = true; }
    if (intent.quality_changed) { value.sr_quality = intent.quality; changed = true; }
    if (intent.dlss_preset_changed) { value.dlss_preset = intent.dlss_preset; changed = true; }
    if (intent.fg_backend_changed) {
        if (!intent.fg_backend) value.fg_enabled = 0;
        else { value.fg_backend = intent.fg_backend; value.fg_enabled = 1; }
        changed = true;
    }
    if (intent.fg_changed) {
        value.fg_enabled = intent.fg_mode != RSF_FG_OFF;
        if (intent.fg_generated) value.generated_frames = intent.fg_generated;
        changed = true;
    }
    if (intent.reflex_changed) { value.latency = intent.reflex_mode; changed = true; }
    if (intent.frame_limit_changed) { value.frame_limit_us = intent.frame_limit_us; changed = true; }
    if (intent.performance_hud_changed) { value.status_overlay = intent.performance_hud != 0; changed = true; }
    return value;
}

int32_t configure(const rsf_overlay_service_setup* setup) noexcept try
{
    if (!setup || setup->struct_size < sizeof(*setup)) return RSF_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_OVERLAY_API_VERSION) return RSF_ERROR_ABI_MISMATCH;
    if (!setup->configuration_path || !*setup->configuration_path || !setup->configuration_section ||
        !*setup->configuration_section || (setup->sr_backend_choices & ~kKnownSr) ||
        (setup->sr_backend_choices && !setup->sr_quality_choices) || (setup->sr_quality_choices & ~kQualities) ||
        (setup->fg_backend_choices & ~kKnownFg) || !has(setup->latency_choices, RSF_REFLEX_OFF) ||
        (setup->latency_choices & ~kLatencies) || !setup->max_generated_frames || setup->max_generated_frames > 15 ||
        setup->defaults.struct_size < sizeof(rsf_render_settings))
        return RSF_ERROR_INVALID_ARGUMENT;
    if (setup->defaults.abi_version != RSF_RENDER_FRONTEND_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    auto& self = service();
    {
        // Choices are checked against the defaults before the one-time claim.
        Service probe;
        probe.defaults = setup->defaults;
        probe.sr_choices = setup->sr_backend_choices; probe.quality_choices = setup->sr_quality_choices;
        probe.fg_choices = setup->fg_backend_choices; probe.latency_choices = setup->latency_choices;
        probe.max_generated = setup->max_generated_frames;
        if (!probe.valid(setup->defaults)) return RSF_ERROR_INVALID_ARGUMENT;
    }
    bool claimed = false;
    std::call_once(self.claimed, [&] { claimed = true; });
    if (!claimed) return RSF_ERROR_BUSY;
    rsf_render_settings settings{};
    {
        std::lock_guard lock(self.guard);
        self.ini = setup->configuration_path; self.section = setup->configuration_section;
        self.identity = setup->identity_utf8 ? setup->identity_utf8 : "adapter";
        self.defaults = setup->defaults; self.defaults.struct_size = sizeof(self.defaults);
        self.sr_choices = setup->sr_backend_choices; self.quality_choices = setup->sr_quality_choices;
        self.fg_choices = setup->fg_backend_choices; self.latency_choices = setup->latency_choices;
        self.max_generated = setup->max_generated_frames; self.preset_available = setup->dlss_preset_available != 0;
        self.logger = setup->log; self.accept = setup->accept_window; self.report = setup->sr_report;
        self.changed = setup->settings_changed; self.user = setup->user;
        const auto saved = rsf::render_settings::read(self.ini, self.section, self.defaults);
        self.settings = self.sanitize(saved);
        settings = self.settings;
        if (!self.valid(saved)) self.say("RSF overlay: saved settings this adapter cannot apply were replaced by its defaults.");
    }
    const auto requested = forward_fg(settings);
    {
        std::lock_guard lock(self.guard); self.request_result = requested;
    }
    self.configured.store(true, std::memory_order_release);
    self.say("RSF overlay service configured for %s; Insert opens the shared panel.", self.identity.c_str());
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }

int32_t present(void* swapchain, uint32_t* drawn) noexcept try
{
    if (drawn) *drawn = 0;
    auto& self = service();
    if (!swapchain) return RSF_ERROR_INVALID_ARGUMENT;
    if (!self.configured.load(std::memory_order_acquire)) return RSF_ERROR_NOT_READY;
    auto* chain = static_cast<IDXGISwapChain*>(swapchain);
    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(chain->GetDesc(&desc)) || !desc.OutputWindow) return RSF_ERROR_INVALID_ARGUMENT;
    if (self.accept && !self.accept(self.user, desc.OutputWindow)) return RSF_ERROR_NOT_READY;
    if (!self.started.load(std::memory_order_acquire)) {
        if (self.start_failed) return RSF_ERROR_NOT_READY;
        if (!rsf_overlay_host_start(swapchain, self.logger, self.user)) {
            self.start_failed = true;
            self.say("RSF overlay: the shared panel did not start for %s; see the preceding overlay lines.", self.identity.c_str());
            return RSF_ERROR_NOT_READY;
        }
        self.started.store(true, std::memory_order_release);
    }
    sample(self, chain, desc);
    rsf_overlay_stats stats{}; stats.struct_size = sizeof(stats);
    rsf_render_settings value{};
    char reason[160]{};
    {
        std::lock_guard lock(self.guard);
        value = self.settings;
        const auto& sr = self.sr;
        if (const char* text = describe(self, false)) std::snprintf(reason, sizeof(reason), "%s", text);
        const bool sr_off = self.sr_unavailable();
        stats.backend_loaded = 1; stats.backend_supported = sr.effective_backend != 0;
        stats.backend_name = sr_name(sr.effective_backend ? sr.effective_backend : sr_off ? RSF_SR_NONE : value.sr_backend);
        stats.render_width = sr.render_width; stats.render_height = sr.render_height;
        stats.output_width = sr.output_width ? sr.output_width : self.chain_width;
        stats.output_height = sr.output_height ? sr.output_height : self.chain_height;
        stats.frames_presented = static_cast<uint32_t>(self.presents);
        stats.frames_evaluated = sr.frames_evaluated; stats.frames_refused = sr.frames_refused;
        stats.last_result = sr.last_result;
        stats.have_scene_color = (sr.inputs & RSF_OVERLAY_SR_INPUT_COLOR) != 0;
        stats.have_depth = (sr.inputs & RSF_OVERLAY_SR_INPUT_DEPTH) != 0;
        stats.have_motion = (sr.inputs & RSF_OVERLAY_SR_INPUT_MOTION) != 0;
        stats.have_exposure = (sr.inputs & RSF_OVERLAY_SR_INPUT_EXPOSURE) != 0;
        stats.motion_decoded = stats.have_motion && sr.effective_backend;
        stats.jitter_active = (sr.inputs & RSF_OVERLAY_SR_INPUT_JITTER) != 0;
        stats.jitter_pixels[0] = sr.jitter_pixels[0]; stats.jitter_pixels[1] = sr.jitter_pixels[1];
        stats.quality = sr.effective_backend ? sr.effective_quality : value.sr_quality;
        stats.enabled = value.sr_enabled;
        stats.backend = sr_off ? RSF_SR_NONE : value.sr_backend; stats.requested_backend = stats.backend;
        stats.last_switch_result = self.apply_result;
        stats.sr_backend_choices = self.sr_choices;
        stats.dlss_preset = value.dlss_preset; stats.dlss_preset_available = self.preset_available;
        stats.sr_quality_locked = sr.quality_locked != 0;
        stats.application_presented_frames = self.presents;
        stats.show_performance_hud = value.status_overlay;
        auto fg = self.fg;
        rsf_overlay_fill_fg_stats(&stats, &fg);
        // Off is the selected choice while disabled; the saved provider is kept for re-enabling.
        stats.fg_backend = self.fg_backend;
        stats.fg_requested_backend = !value.fg_enabled ? 0 :
            value.fg_backend == RSF_FG_BACKEND_AUTO ? stats.fg_backend : value.fg_backend;
        if (!value.fg_enabled && fg.vendor.configured_mode == RSF_FG_OFF) stats.fg_backend = 0;
        stats.fg_backend_choices = (self.fg_choices & (self.bridge_choices | (1u << RSF_FG_BACKEND_AUTO))) | 1u |
            (self.fg_owner ? RSF_OVERLAY_FG_RUNTIME_SWITCH : 0u);
        stats.fg_requested_mode = value.fg_enabled ? RSF_FG_FIXED : RSF_FG_OFF;
        stats.fg_requested_generated = value.generated_frames;
        stats.reflex_requested_mode = value.latency; stats.frame_limit_us = value.frame_limit_us;
        stats.fg_selection_result = self.request_result ? self.request_result : self.switch_result;
    }
    stats.refusal_reason = reason[0] ? reason : nullptr;
    LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    stats.sample_qpc = static_cast<uint64_t>(now.QuadPart); stats.qpc_frequency = static_cast<uint64_t>(frequency.QuadPart);
    if (rsf_overlay_host_visible()) stats.display_refresh_mhz = display_refresh(self, desc.OutputWindow);
    rsf_overlay_intent intent{}; intent.struct_size = sizeof(intent);
    if (!rsf_overlay_host_present(swapchain, &stats, &intent)) return RSF_OK;
    if (drawn) *drawn = 1;
    { std::lock_guard lock(self.guard); ++self.presents_drawn; }
    bool changed = false;
    const auto requested = with_intent(value, intent, changed);
    if (changed) {
        const auto result = apply_logged(self, requested, "panel");
        { std::lock_guard lock(self.guard); self.apply_result = result; }
        if (result != RSF_OK) self.say("RSF overlay: panel request refused (%d); saved settings retained.", result);
    }
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }

int32_t notify(void*, uint32_t reason) noexcept try
{
    auto& self = service();
    if (reason != RSF_OVERLAY_NOTIFY_RESIZE && reason != RSF_OVERLAY_NOTIFY_DEVICE_RESET) return RSF_ERROR_INVALID_ARGUMENT;
    if (!self.configured.load(std::memory_order_acquire)) return RSF_ERROR_NOT_READY;
    if (reason == RSF_OVERLAY_NOTIFY_RESIZE) {
        std::lock_guard lock(self.guard); self.chain_width = self.chain_height = 0;
        return RSF_OK;
    }
    if (self.started.exchange(false, std::memory_order_acq_rel)) {
        rsf_overlay_host_stop();
        self.say("RSF overlay: released for a chain/device reset; it restarts at the next accepted Present.");
    }
    self.start_failed = false;
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }

uint32_t visible() noexcept
{
    return service().started.load(std::memory_order_acquire) ? rsf_overlay_host_visible() : 0u;
}

int32_t set_visible(uint32_t requested) noexcept try
{
    if (!service().started.load(std::memory_order_acquire)) return RSF_ERROR_NOT_READY;
    rsf_overlay_input_set_visible(requested ? 1u : 0u);
    return (rsf_overlay_host_visible() != 0) == (requested != 0) ? RSF_OK : RSF_ERROR_BUSY;
} catch (...) { return RSF_ERROR_NOT_READY; }

int32_t toggle() noexcept { return set_visible(visible() ? 0u : 1u); }

int32_t capture(rsf_overlay_capture* out) noexcept
{
    if (!out || out->struct_size < sizeof(*out)) return RSF_ERROR_INVALID_ARGUMENT;
    const uint32_t started = service().started.load(std::memory_order_acquire) ? 1u : 0u;
    const uint32_t open = started ? rsf_overlay_host_visible() : 0u;
    out->struct_size = sizeof(*out);
    out->started = started; out->visible = open;
    out->captured = open ? RSF_OVERLAY_CAPTURE_MOUSE | RSF_OVERLAY_CAPTURE_KEYBOARD : 0u;
    return RSF_OK;
}

int32_t status(rsf_render_status* output) noexcept try
{
    if (!output || output->struct_size < sizeof(*output)) return RSF_ERROR_INVALID_ARGUMENT;
    if (output->abi_version != RSF_RENDER_FRONTEND_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    auto& self = service();
    if (!self.configured.load(std::memory_order_acquire)) return RSF_ERROR_NOT_READY;
    rsf_render_status value{}; value.struct_size = sizeof(value); value.abi_version = RSF_RENDER_FRONTEND_ABI_VERSION;
    DWORD foreground{}; GetWindowThreadProcessId(GetForegroundWindow(), &foreground);
    std::lock_guard lock(self.guard);
    const auto& wanted = self.settings;
    const auto& sr = self.sr;
    const auto& fg = self.fg;
    value.sr_enabled = wanted.sr_enabled; value.sr_requested_backend = self.sr_unavailable() ? RSF_SR_NONE : wanted.sr_backend;
    value.sr_effective_backend = sr.effective_backend;
    value.render_width = sr.render_width; value.render_height = sr.render_height;
    value.output_width = sr.output_width ? sr.output_width : self.chain_width;
    value.output_height = sr.output_height ? sr.output_height : self.chain_height;
    value.fg_requested = wanted.fg_enabled;
    value.fg_requested_backend = wanted.fg_enabled ? wanted.fg_backend : 0;
    value.fg_effective_backend = self.fg_owner && fg.vendor.configured_mode != RSF_FG_OFF ? self.fg_backend : 0;
    value.fg_active = self.fg_owner ? fg.vendor.active : 0;
    value.requested_generated = wanted.generated_frames;
    value.effective_generated = self.fg_owner ? fg.vendor.effective_generated_frames : 0;
    value.max_generated = fg.vendor.max_generated_frames < self.max_generated ? fg.vendor.max_generated_frames : self.max_generated;
    value.foreground = foreground == GetCurrentProcessId();
    value.latency_requested = wanted.latency;
    value.latency_effective = self.fg_owner && fg.vendor.low_latency_available ? fg.vendor.effective_reflex : RSF_REFLEX_OFF;
    value.fg_reason = fg.reason; value.sr_result = sr.last_result;
    value.fg_result = self.request_result ? self.request_result : fg.last_result;
    value.fg_vendor_result = fg.vendor.vendor_status;
    value.output_fps_valid = self.output_fps_valid; value.status_overlay = wanted.status_overlay;
    value.sr_backend_choices = self.sr_choices;
    value.fg_backend_choices = self.fg_choices & (self.bridge_choices | (1u << RSF_FG_BACKEND_AUTO));
    value.base_fps = self.base_fps; value.output_fps = self.output_fps;
    value.source_frame = fg.source_frame_id; value.tagged_frames = fg.tagged_frames;
    value.application_presents = self.presents; value.output_presents = fg.vendor.total_presented;
    std::snprintf(value.reason, sizeof(value.reason), "%s", describe(self, true));
    *output = value;
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }

int32_t settings_get(rsf_render_settings* output) noexcept try
{
    if (!output || output->struct_size < sizeof(*output)) return RSF_ERROR_INVALID_ARGUMENT;
    if (output->abi_version != RSF_RENDER_FRONTEND_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    auto& self = service();
    if (!self.configured.load(std::memory_order_acquire)) return RSF_ERROR_NOT_READY;
    std::lock_guard lock(self.guard);
    *output = self.settings;
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }

int32_t settings_apply(const rsf_render_settings* request) noexcept try
{
    if (!request) return RSF_ERROR_INVALID_ARGUMENT;
    return apply_logged(service(), *request, "api");
} catch (...) { return RSF_ERROR_NOT_READY; }
}

extern "C" RSF_RUNTIME_API rsf_result rsf_get_overlay_diagnostics(rsf_overlay_diagnostics* out)
{
    if (!out || out->struct_size < sizeof(*out)) return RSF_ERROR_INVALID_ARGUMENT;
    auto& self = service();
    rsf_overlay_diagnostics value{};
    value.struct_size = sizeof(value);
    value.started = self.started.load(std::memory_order_acquire) ? 1u : 0u;
    value.visible = value.started ? rsf_overlay_host_visible() : 0u;
    {
        std::lock_guard lock(self.guard);
        value.presents = self.presents; value.presents_drawn = self.presents_drawn;
        value.chain_width = self.chain_width; value.chain_height = self.chain_height;
        value.apply_attempts = self.apply_attempts; value.apply_refused = self.apply_refused;
        value.last_apply_result = self.last_apply_status;
    }
    rsf_overlay_input_counters input{}; input.struct_size = sizeof(input);
    if (rsf_overlay_input_counters_get(&input) == RSF_OVERLAY_INPUT_OK) {
        value.input_installed = input.installed; value.cursor_freed = input.cursor_freed;
        value.pointer_received = input.pointer_received; value.pointer_consumed = input.pointer_consumed;
        value.button_downs_consumed = input.button_downs_consumed; value.wheel_consumed = input.wheel_consumed;
        value.raw_mouse_reports = input.raw_mouse_reports; value.keys_consumed = input.keys_consumed;
        value.frames_visible = input.frames_visible; value.frames_with_button = input.frames_with_button;
        value.last_pointer_x = input.last_pointer_x; value.last_pointer_y = input.last_pointer_y;
    }
    *out = value;
    return RSF_OK;
}

extern "C" RSF_RUNTIME_API rsf_result rsf_get_overlay_api(rsf_overlay_api* api)
{
    if (!api || api->struct_size < sizeof(*api)) return RSF_ERROR_INVALID_ARGUMENT;
    if (api->abi_version != RSF_OVERLAY_API_VERSION) return RSF_ERROR_ABI_MISMATCH;
    const rsf_overlay_api complete{
        sizeof(rsf_overlay_api),
        RSF_OVERLAY_API_VERSION,
        &configure,
        &present,
        &notify,
        &visible,
        &set_visible,
        &toggle,
        &capture,
        &status,
        &settings_get,
        &settings_apply,
    };
    *api = complete;
    return RSF_OK;
}
