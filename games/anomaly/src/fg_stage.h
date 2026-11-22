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
#include <rescaleframe/game_api.h>
#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C"
#endif
bool rsf_anomaly_fg_configure_native(const wchar_t*, bool apply, void* runtime, uint32_t depth_rva,
    rsf_game_log_fn)
#ifdef __cplusplus
    noexcept
#endif
    ;
