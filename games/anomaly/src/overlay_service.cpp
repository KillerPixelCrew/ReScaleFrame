// SPDX-License-Identifier: GPL-3.0-only
#include "overlay_service.h"
#include <rescaleframe/frame_generation.h>
#include <rescaleframe/sr_session.h>
#include <d3d11.h>
#include <atomic>
#include <cstdarg>
#include <cstdio>
#include <cwchar>
#include <filesystem>
#include <string>

static_assert(rsf::anomaly::overlay::capture_mouse == RSF_OVERLAY_CAPTURE_MOUSE);
static_assert(rsf::anomaly::overlay::capture_keyboard == RSF_OVERLAY_CAPTURE_KEYBOARD);

namespace rsf::anomaly::overlay {
namespace {
constexpr wchar_t engine_window_class[] = L"_XRAY_1.5";  // xrEngine Device_Initialize.cpp
constexpr wchar_t settings_section[] = L"Anomaly";

uint32_t capture_bits(void*) noexcept;

// What a run needs in order to judge the panel without desktop capture. Present-thread only.
struct CaptureConfig {
    bool enabled{};
    uint32_t frames{3}, stride{20}, cycles{2};
    std::string directory;   // UTF-8
};
struct PanelRun {
    bool visible{};
    uint32_t opens{}, closes{};
    uint64_t frames{}, drawn{}, refused{};
    int32_t last_result{};
    uint32_t summaries{};
    rsf_overlay_diagnostics base{};
    bool have_base{};
    // Capture bookkeeping for the phase (open or close) that began at the last visibility edge.
    bool phase_open{};
    bool phase_active{};
    uint32_t phase_frame{}, phase_taken{}, phase_cycle{};
};

struct State {
    rsf_overlay_api api{};
    rsf_get_overlay_diagnostics_fn diagnostics{};
    rsf_input_dump_named_texture_fn capture_fn{};
    CaptureConfig capture;
    PanelRun run;
    rsf_game_log_fn log{};
    void* log_user{};
    bool key_poll{true}, insert_down{};
    // The chain/device the panel is attached to. Compared only, never dereferenced.
    const void* chain{};
    const void* device{};
    std::atomic<bool> started{};
    InputGate gate{&capture_bits, nullptr};
};
State& state() noexcept { static auto* value = new State; return *value; }

void say(const char* format, ...) noexcept {
    auto& s = state();
    if (!s.log) return;
    char text[768];
    va_list arguments; va_start(arguments, format);
    const int written = std::vsnprintf(text, sizeof(text), format, arguments);
    va_end(arguments);
    if (written >= 0) try { s.log(s.log_user, text); } catch (...) {}
}

uint32_t capture_bits(void*) noexcept { return query_capture(); }

int accept_window(void*, void* window) noexcept { return is_engine_window(static_cast<HWND>(window)) ? 1 : 0; }

void settings_changed(void*, const rsf_render_settings* applied) noexcept {
    if (!applied) return;
    say("Anomaly overlay settings applied: SR=%u SRBackend=%u SRQuality=%u FG=%u StatusOverlay=%u Latency=%u",
        applied->sr_enabled, applied->sr_backend, applied->sr_quality, applied->fg_enabled,
        applied->status_overlay, applied->latency);
}

bool read_diagnostics(rsf_overlay_diagnostics& out) noexcept {
    auto& s = state();
    out = {};
    out.struct_size = sizeof(out);
    return s.diagnostics && s.diagnostics(&out) == RSF_OK;
}

void say_panel_summary(const char* when) noexcept {
    auto& r = state().run;
    rsf_overlay_diagnostics now{};
    if (!read_diagnostics(now) || !r.have_base) {
        say("Anomaly overlay panel #%u %s: present drawn %llu of %llu visible frames (%llu refused, last result %d); "
            "input and settings counters unavailable.", r.opens, when, static_cast<unsigned long long>(r.drawn),
            static_cast<unsigned long long>(r.frames), static_cast<unsigned long long>(r.refused), r.last_result);
        return;
    }
    const auto& b = r.base;
    auto d = [](uint64_t a, uint64_t c) { return static_cast<unsigned long long>(a - c); };
    say("Anomaly overlay panel #%u %s: present drawn %llu of %llu visible frames (%llu refused, last result %d), target %ux%u; "
        "pointer events received %llu consumed %llu (button downs %llu, wheel %llu), raw mouse reports %llu, keys consumed %llu; "
        "input frames sampled %llu with a button held %llu, last pointer (%u,%u), cursor freed %u; "
        "settings_apply attempts %u refused %u last result %d.",
        r.opens, when, static_cast<unsigned long long>(r.drawn), static_cast<unsigned long long>(r.frames),
        static_cast<unsigned long long>(r.refused), r.last_result, now.chain_width, now.chain_height,
        d(now.pointer_received, b.pointer_received), d(now.pointer_consumed, b.pointer_consumed),
        d(now.button_downs_consumed, b.button_downs_consumed), d(now.wheel_consumed, b.wheel_consumed),
        d(now.raw_mouse_reports, b.raw_mouse_reports), d(now.keys_consumed, b.keys_consumed),
        d(now.frames_visible, b.frames_visible), d(now.frames_with_button, b.frames_with_button),
        now.last_pointer_x, now.last_pointer_y, now.cursor_freed,
        now.apply_attempts - b.apply_attempts, now.apply_refused - b.apply_refused, now.last_apply_result);
}

// Writes the presented backbuffer, after the panel drew into it, through the Runtime's frame dump service.
void capture_backbuffer(IDXGISwapChain* chain, const char* phase, uint32_t cycle, uint32_t index) noexcept {
    auto& s = state();
    ID3D11Device* device = nullptr;
    ID3D11DeviceContext* context = nullptr;
    ID3D11Texture2D* buffer = nullptr;
    if (SUCCEEDED(chain->GetDevice(__uuidof(ID3D11Device), reinterpret_cast<void**>(&device))) && device)
        device->GetImmediateContext(&context);
    if (context) (void)chain->GetBuffer(0, __uuidof(ID3D11Texture2D), reinterpret_cast<void**>(&buffer));
    int32_t result = -1 /* no backbuffer or context */;
    char name[48];
    std::snprintf(name, sizeof(name), "%s%02u_f%02u", phase, cycle, index);
    rsf_named_frame_dump_report report{sizeof(report)};
    if (buffer) {
        rsf_named_frame_dump request{};
        request.struct_size = sizeof(request);
        request.abi_version = RSF_NAMED_FRAME_DUMP_ABI_VERSION;
        request.directory_utf8 = s.capture.directory.c_str();
        request.name = name;
        request.view = 0;   // the backbuffer is display-referred: channels written through
        request.scale = 1;
        result = s.capture_fn(context, buffer, &request, &report);
    }
    say("Anomaly overlay capture %s: result %d (%ux%u format %u) -> %s", name, result, report.width, report.height,
        report.dxgi_format, s.capture.directory.c_str());
    if (buffer) buffer->Release();
    if (context) context->Release();
    if (device) device->Release();
}

void track_panel(IDXGISwapChain* chain, const DXGI_SWAP_CHAIN_DESC& desc, int32_t presented, uint32_t drawn) noexcept {
    auto& s = state();
    auto& r = s.run;
    const bool visible = s.api.visible() != 0;
    const bool rising = visible && !r.visible, falling = !visible && r.visible;
    if (falling) {
        say_panel_summary("closed");
        // The Runtime counts input only while the panel is open, so the next panel's counts start where this
        // one ended; events that arrive between the open request and its first presented frame still count.
        rsf_overlay_diagnostics now{};
        if (read_diagnostics(now)) r.base = now;
    }
    if (rising) {
        ++r.opens; r.frames = r.drawn = r.refused = 0; r.summaries = 0; r.last_result = 0;
        rsf_overlay_diagnostics now{};
        r.have_base = s.diagnostics != nullptr;   // the first panel's baseline is the zeroed counters of process start
        (void)read_diagnostics(now);
        say("Anomaly overlay panel #%u opened: target %ux%u, input layer installed %u, cursor freed %u.", r.opens,
            desc.BufferDesc.Width, desc.BufferDesc.Height, now.input_installed, now.cursor_freed);
    }
    if (falling) ++r.closes;
    if (rising || falling) {
        r.phase_open = rising; r.phase_active = true; r.phase_frame = r.phase_taken = 0;
        r.phase_cycle = rising ? r.opens : r.closes;
    }
    if (visible) {
        ++r.frames;
        r.drawn += drawn ? 1u : 0u;
        r.last_result = presented;
        if (presented != RSF_OK || !drawn) ++r.refused;
        // The first frames in full; after that a bounded progress line so a crash while open still leaves counters.
        if (r.frames <= 3)
            say("Anomaly overlay panel #%u frame %llu: api.present result %d, drawn %u, target %ux%u.", r.opens,
                static_cast<unsigned long long>(r.frames), presented, drawn, desc.BufferDesc.Width, desc.BufferDesc.Height);
        else if (r.summaries < 6 && (r.frames == 60 || r.frames % 600 == 0)) { ++r.summaries; say_panel_summary("progress"); }
    }
    if (r.phase_active && s.capture.enabled && s.capture_fn && r.phase_cycle <= s.capture.cycles) {
        if (r.phase_taken < s.capture.frames && r.phase_frame % s.capture.stride == 0) {
            capture_backbuffer(chain, r.phase_open ? "open" : "close", r.phase_cycle, r.phase_taken);
            ++r.phase_taken;
        }
        ++r.phase_frame;
        if (r.phase_taken >= s.capture.frames) r.phase_active = false;
    } else if (r.phase_active && !(s.capture.enabled && s.capture_fn)) r.phase_active = false;
    r.visible = visible;
}

void on_present(void*, IDXGISwapChain* chain) noexcept {
    auto& s = state();
    if (!s.started.load(std::memory_order_acquire) || !chain) return;
    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(chain->GetDesc(&desc)) || !desc.OutputWindow || !is_engine_window(desc.OutputWindow)) return;
    ID3D11Device* device = nullptr;
    (void)chain->GetDevice(__uuidof(ID3D11Device), reinterpret_cast<void**>(&device));
    const void* device_identity = device;
    if (device) device->Release();
    // A different chain or device than the panel drew with: release it before it draws on stale objects.
    if ((s.chain && s.chain != chain) || (s.device && s.device != device_identity)) {
        (void)s.api.notify(chain, RSF_OVERLAY_NOTIFY_DEVICE_RESET);
        say("Anomaly overlay: the engine presented on a new swap chain or device; the panel restarts on it.");
    }
    s.chain = chain; s.device = device_identity;
    uint32_t drawn = 0;
    const int32_t presented = s.api.present(chain, &drawn);
    track_panel(chain, desc, presented, drawn);
    if (s.key_poll) {
        // Insert normally arrives as a window message. If DirectInput's focus handling swallows it, this
        // edge toggles instead; the Runtime's 250 ms debounce makes the two paths safe together.
        const bool down = (GetAsyncKeyState(VK_INSERT) & 0x8000) != 0 && GetForegroundWindow() == desc.OutputWindow;
        if (down && !s.insert_down) (void)s.api.toggle();
        s.insert_down = down;
    }
}
void on_resize(void*, IDXGISwapChain* chain) noexcept {
    auto& s = state();
    if (s.started.load(std::memory_order_acquire)) (void)s.api.notify(chain, RSF_OVERLAY_NOTIFY_RESIZE);
}

// What this adapter can apply today: nothing of SR or FG. Zero SR choices tell the Runtime that SR is
// unavailable, so the panel offers none, a request to enable it is refused and status says so.
rsf_overlay_service_setup make_setup(const StartOptions& options) noexcept {
    rsf_overlay_service_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_OVERLAY_API_VERSION;
    setup.identity_utf8 = "Anomaly";
    setup.configuration_path = options.configuration;
    setup.configuration_section = settings_section;
    setup.defaults = {sizeof(rsf_render_settings), RSF_RENDER_FRONTEND_ABI_VERSION};
    setup.defaults.sr_enabled = 0;
    setup.defaults.sr_backend = RSF_SR_NONE;
    setup.defaults.fg_enabled = 0;
    setup.defaults.fg_backend = RSF_FG_BACKEND_AUTO;
    setup.defaults.generated_frames = 1;
    setup.defaults.latency = RSF_REFLEX_OFF;
    setup.defaults.status_overlay = 0;
    setup.defaults.sr_quality = 0;
    setup.sr_backend_choices = 0;
    setup.sr_quality_choices = 0;
    setup.fg_backend_choices = 0;
    setup.latency_choices = 1u << RSF_REFLEX_OFF;
    setup.max_generated_frames = 1;
    setup.dlss_preset_available = 0;
    setup.log = options.log;
    setup.accept_window = &accept_window;
    setup.sr_report = nullptr;
    setup.settings_changed = &settings_changed;
    setup.user = options.log_user;
    return setup;
}
}

namespace {
std::string to_utf8(const std::wstring& text) {
    if (text.empty()) return {};
    const int size = WideCharToMultiByte(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), nullptr, 0, nullptr, nullptr);
    std::string out(size > 0 ? static_cast<size_t>(size) : 0u, '\0');
    if (size > 0) WideCharToMultiByte(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), out.data(), size, nullptr, nullptr);
    return out;
}

// Always resolves the diagnostics (they cost nothing); the PNG capture is opt-in.
void configure_capture(const StartOptions& options) noexcept {
    auto& s = state();
    s.diagnostics = options.diagnostics_override;
    s.capture_fn = options.capture_override;
    if (options.runtime) {
        if (!s.diagnostics) s.diagnostics = reinterpret_cast<rsf_get_overlay_diagnostics_fn>(
            reinterpret_cast<void*>(GetProcAddress(options.runtime, RSF_OVERLAY_DIAGNOSTICS_ENTRY_POINT)));
        if (!s.capture_fn) s.capture_fn = reinterpret_cast<rsf_input_dump_named_texture_fn>(
            reinterpret_cast<void*>(GetProcAddress(options.runtime, RSF_INPUT_DUMP_NAMED_TEXTURE_ENTRY_POINT)));
    }
    if (!s.diagnostics) say("Anomaly overlay: the Runtime has no rsf_get_overlay_diagnostics; panel logs omit input and settings counters.");
    auto& c = s.capture;
    c = {};
    if (GetPrivateProfileIntW(settings_section, L"OverlayCapture", 0, options.configuration) == 0) return;
    if (!s.capture_fn) { say("Anomaly overlay: OverlayCapture=1 but the Runtime has no rsf_input_dump_named_texture; no PNGs will be written."); return; }
    auto bounded = [&](const wchar_t* key, int fallback, int low, int high) {
        const int value = static_cast<int>(GetPrivateProfileIntW(settings_section, key, fallback, options.configuration));
        return static_cast<uint32_t>(value < low ? low : value > high ? high : value);
    };
    c.frames = bounded(L"OverlayCaptureFrames", 3, 1, 16);
    c.stride = bounded(L"OverlayCaptureStride", 20, 1, 600);
    c.cycles = bounded(L"OverlayCaptureCycles", 2, 1, 8);
    wchar_t custom[MAX_PATH]{};
    GetPrivateProfileStringW(settings_section, L"OverlayCaptureDir", L"", custom, static_cast<DWORD>(_countof(custom)), options.configuration);
    std::filesystem::path directory = custom;
    if (directory.empty()) {
        SYSTEMTIME t{}; GetLocalTime(&t);
        wchar_t stamp[32];
        std::swprintf(stamp, _countof(stamp), L"%04u%02u%02u-%02u%02u%02u", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond);
        directory = std::filesystem::path(options.configuration).parent_path() / L"RSF-OverlayCapture" / stamp;
    }
    c.directory = to_utf8(directory.wstring());
    c.enabled = true;
    say("Anomaly overlay capture armed: %u frames every %u presents after each of the first %u opens and closes -> %s",
        c.frames, c.stride, c.cycles, c.directory.c_str());
}
}

StartStatus bind_api(rsf_get_overlay_api_fn query, rsf_overlay_api* out, const char** reason) noexcept {
    const char* unused = nullptr;
    if (!reason) reason = &unused;
    if (!query || !out) { *reason = "the Runtime does not export rsf_get_overlay_api"; return StartStatus::api_missing; }
    rsf_overlay_api table{};
    table.struct_size = sizeof(table);
    table.abi_version = RSF_OVERLAY_API_VERSION;
    const rsf_result result = query(&table);
    if (result == RSF_ERROR_ABI_MISMATCH) { *reason = "the Runtime's overlay API version does not match"; return StartStatus::api_refused; }
    if (result != RSF_OK) { *reason = "the Runtime refused the overlay API query"; return StartStatus::api_refused; }
    if (table.struct_size < sizeof(table) || table.abi_version != RSF_OVERLAY_API_VERSION) {
        *reason = "the Runtime's overlay API table is short or of another version"; return StartStatus::api_refused;
    }
    if (!table.configure || !table.present || !table.notify || !table.visible || !table.set_visible || !table.toggle ||
        !table.capture || !table.status || !table.settings_get || !table.settings_apply) {
        *reason = "the Runtime's overlay API table has a missing entry"; return StartStatus::api_refused;
    }
    *out = table;
    return StartStatus::started;
}

bool is_engine_window(HWND window) noexcept {
    if (!window || !IsWindow(window)) return false;
    DWORD process = 0;
    GetWindowThreadProcessId(window, &process);
    if (process != GetCurrentProcessId()) return false;
    wchar_t name[32]{};
    return GetClassNameW(window, name, static_cast<int>(_countof(name))) > 0 && !std::wcscmp(name, engine_window_class);
}

uint32_t query_capture() noexcept {
    auto& s = state();
    if (!s.api.capture) return 0;
    rsf_overlay_capture capture{};
    capture.struct_size = sizeof(capture);
    return s.api.capture(&capture) == RSF_OK ? capture.captured : 0u;
}

PresentSink present_sink() noexcept { return {&on_present, &on_resize, nullptr}; }
InputGate& input_gate() noexcept { return state().gate; }

StartStatus start(const StartOptions& options, const char** reason) noexcept try {
    const char* unused = nullptr;
    if (!reason) reason = &unused;
    auto& s = state();
    if (s.started.load(std::memory_order_acquire) || s.api.configure) { *reason = "the overlay was already started"; return StartStatus::already_started; }
    if (!options.configuration || !*options.configuration) { *reason = "no adapter configuration path"; return StartStatus::configure_refused; }
    s.log = options.log; s.log_user = options.log_user;
    rsf_overlay_api api{};
    StartStatus bound{};
    if (options.api_override) {
        // Pass the override through the same validation as a Runtime table.
        static const rsf_overlay_api* injected;
        injected = options.api_override;
        bound = bind_api([](rsf_overlay_api* table) -> rsf_result { *table = *injected; return RSF_OK; }, &api, reason);
    } else {
        auto query = options.runtime ? reinterpret_cast<rsf_get_overlay_api_fn>(
            reinterpret_cast<void*>(GetProcAddress(options.runtime, RSF_OVERLAY_API_ENTRY_POINT))) : nullptr;
        bound = bind_api(query, &api, reason);
    }
    if (bound != StartStatus::started) { say("Anomaly overlay refused: %s.", *reason); return bound; }
    const auto setup = make_setup(options);
    const int32_t configured = api.configure(&setup);
    if (configured != RSF_OK) {
        *reason = "the Runtime rejected the Anomaly overlay configuration";
        say("Anomaly overlay refused: %s (result=%d).", *reason, configured);
        return StartStatus::configure_refused;
    }
    s.api = api;
    s.key_poll = GetPrivateProfileIntW(settings_section, L"OverlayKeyPoll", 1, options.configuration) != 0;
    configure_capture(options);
    rsf_render_settings saved{sizeof(saved), RSF_RENDER_FRONTEND_ABI_VERSION};
    if (api.settings_get(&saved) == RSF_OK)
        say("Anomaly overlay configured: saved SR=%u FG=%u StatusOverlay=%u. SR and FG are not wired in this adapter; "
            "the panel offers FG Off only and reports SR as unavailable.", saved.sr_enabled, saved.fg_enabled, saved.status_overlay);
    if (options.install_hooks) {
        // Input first: a panel that cannot keep the game from acting on mouse and keyboard must not open.
        if (!install_input_hooks(s.gate, reason)) {
            say("Anomaly overlay refused: DirectInput gate unavailable (%s). The panel will not start.", *reason);
            return StartStatus::hooks_refused;
        }
        if (!install_present_hooks(present_sink(), reason)) {
            say("Anomaly overlay refused: Present hook unavailable (%s). The panel will not start.", *reason);
            return StartStatus::hooks_refused;
        }
    }
    s.started.store(true, std::memory_order_release);
    say("Anomaly overlay active: Insert opens the shared panel; mouse and keyboard are withheld from the game while it is open.");
    return StartStatus::started;
} catch (...) { if (reason) *reason = "contained C++ exception"; return StartStatus::configure_refused; }
}
