// SPDX-License-Identifier: GPL-3.0-only
// Anomaly's use of the Runtime's shared overlay (rsf_get_overlay_api). Everything the Runtime must
// not know lives here: the engine window class, the Present and DirectInput boundaries, what this
// adapter can and cannot apply. The service never loads a second Runtime; it queries the one the
// carrier already loaded.
#pragma once
#include "overlay_hooks.h"
#include "overlay_input_gate.h"
#include <rescaleframe/overlay_api.h>
#include <windows.h>

namespace rsf::anomaly::overlay {
enum class StartStatus {
    started,
    already_started,
    api_missing,         // the Runtime does not export rsf_get_overlay_api
    api_refused,         // version mismatch, short table, or a missing entry
    configure_refused,   // the Runtime rejected the adapter's setup
    hooks_refused,       // the guarded Present/DirectInput hooks could not be installed
};

struct StartOptions {
    HMODULE runtime{};                       // the carrier-loaded ReScaleFrame.Runtime.dll
    const wchar_t* configuration{};          // absolute adapter INI; settings live in [Anomaly]
    rsf_game_log_fn log{};
    void* log_user{};
    // Test seams. Production leaves both at their defaults.
    const rsf_overlay_api* api_override{};   // used instead of resolving the Runtime export
    bool install_hooks{true};
};

// Validates a table obtained from `query`. On success fills `out`; on failure `out` is untouched and
// `reason` names the refusal. Null `query` is "missing".
StartStatus bind_api(rsf_get_overlay_api_fn query, rsf_overlay_api* out, const char** reason) noexcept;

// Cold start, outside DllMain, before the first Present. Refuses cleanly and installs no hook unless
// the API binds and the Runtime accepts the configuration. One call per process.
// INI keys: [Anomaly] Overlay=1 selects this (read by the caller); OverlayKeyPoll=0 disables the
// adapter's own Insert fallback; the shared render settings (SR, FG, StatusOverlay, ...) are written
// by the Runtime into the same section.
StartStatus start(const StartOptions& options, const char** reason) noexcept;

// Window filter given to the Runtime: the engine's main window in this process.
bool is_engine_window(HWND window) noexcept;

// The capture bits (RSF_OVERLAY_CAPTURE_*) the input gate acts on; 0 before start or when the query fails.
uint32_t query_capture() noexcept;

// The Present/ResizeBuffers sink the hooks call. Exposed so a contract can drive it with a real chain.
PresentSink present_sink() noexcept;

// The gate DirectInput devices created after start are registered with.
InputGate& input_gate() noexcept;
}
