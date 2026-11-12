// SPDX-License-Identifier: GPL-3.0-only
#include "overlay_service.h"
#include <rescaleframe/frame_generation.h>
#include <rescaleframe/sr_session.h>
#include <d3d11.h>
#include <atomic>
#include <cstdarg>
#include <cstdio>
#include <cwchar>

static_assert(rsf::anomaly::overlay::capture_mouse == RSF_OVERLAY_CAPTURE_MOUSE);
static_assert(rsf::anomaly::overlay::capture_keyboard == RSF_OVERLAY_CAPTURE_KEYBOARD);

namespace rsf::anomaly::overlay {
namespace {
constexpr wchar_t engine_window_class[] = L"_XRAY_1.5";  // xrEngine Device_Initialize.cpp
constexpr wchar_t settings_section[] = L"Anomaly";

uint32_t capture_bits(void*) noexcept;

struct State {
    rsf_overlay_api api{};
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
    char text[320];
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
    (void)s.api.present(chain, &drawn);
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
