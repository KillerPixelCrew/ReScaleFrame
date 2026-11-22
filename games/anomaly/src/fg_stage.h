// SPDX-License-Identifier: MIT
#pragma once
// Frame generation for Anomaly through the Runtime's shared D3D11 presentation bridge (rsf_get_d3d11_present_api)
// and its D3D11 FG tagging path (rsf_get_d3d11_fg_api). The adapter owns the engine boundaries, the screen policy
// (fg_policy.rs), the HUD-less boundary and the SR-off depth/motion pass (fg_inputs.rs). The Runtime owns the vendor
// providers, presentation, pacing, latency markers, shared surfaces and their retirement.
//
// INI ([Anomaly], all optional except FG):
//   FG=1                  generate frames from startup. FG=1 or FGBridge=1 installs the bridge at cold start (before
//                         device creation; it cannot be added later). The overlay's FG switch then toggles generation.
//   FGBridge              default 1: install the bridge with FG initially off (the overlay can turn it on); 0 keeps
//                         presentation stock unless FG=1
//   FGBackend=7           1 DLSS-G, 3 FSR3, 4 FSR4, 5 XeSS, 7 Auto (by the rendering adapter's vendor)
//   FGGeneratedFrames=1   generated frames per rendered frame, clamped by the provider (1..3)
//   Latency=1             0 off, 1 on, 2 boost (Reflex for DLSS-G; the provider's own pacing otherwise)
//   FrameLimitUs=0        minimum interval between rendered frames, before generation; 0 unlimited
//   FGScopeHold=4         engine frames held back after a scope (secondary viewport) frame
//   FGDebug=0             Runtime FG eligibility/pacing log lines
//   FGDevelopment=0       load the vendors' development runtimes
//   FGCapture=1           dump the FG inputs (HUD-less colour, depth, motion) through the frame dump service for
//                         FGCaptureCount (default 3, max 16) fed frames from FGCaptureStart (default 600), every
//                         FGCaptureInterval (default 300) engine frames, into FGCaptureDir
//                         (default <ini directory>\RSF-FGCapture\<local time>)
//   SRStreamlineDir / SRFidelityFXDir / SRXeSSDir   vendor runtimes, shared with SR
// The shared frontend keys (FG, FGBackend, FGGeneratedFrames, Latency, FrameLimitUs) are also what the overlay saves.
//
// The native render and input callbacks run on the engine thread.
#include "build_profile.h"
#include "fg_stage_platform.h"
#include <rescaleframe/game_api.h>
#include <rescaleframe/d3d11_fg_api.h>
#include <rescaleframe/d3d11_present_api.h>
#include <windows.h>
#include <cstdint>

namespace rsf::anomaly::fg {
// Cold-start admission uses the exact BuildProfile selected by the native carrier.
bool configure_ini(const wchar_t* ini, bool apply, HMODULE runtime, const BuildProfile& build,
    rsf_game_log_fn log) noexcept;

// The native bootstrap provides the overlay sink once. The engine-facing
// swap chain is borrowed during the bridge's before_present callback.
inline void set_overlay_sink(rsf_anomaly_fg_overlay_present sink) noexcept {
    rsf_anomaly_fg_set_overlay_sink(sink, nullptr);
}

inline constexpr auto installed = rsf_anomaly_fg_installed;

// Preserve the bootstrap callback signatures also declared by input_hooks.h.
// OnFrame still reports input through the guarded native read and unwind boundary.
inline void input_begin(uint32_t engine_frame) noexcept {
    rsf_anomaly_fg_stage_input_begin(engine_frame);
}
inline void input_received(uint32_t kinds, uint32_t message) noexcept {
    rsf_anomaly_fg_stage_input_received(kinds, message);
}
}
