// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// SR stages behind [Anomaly] SRStage (docs/research/anomaly-sr-seam.md sections 9 to 11). Opt-in, Observe mode only.
//   SRStage=1  generic0/generic_temp FP16 twins at output size, f^-1/f round trip at the seam.
//   SRStage=2  every world target at render size (SRScale, default 0.67, clamped to [0.5, 1.0]), Device size and
//              HW.pBaseZB swapped for the world window, bilinear upscale in linear space at the seam.
//   SRStage=3  Stage 2 plus a Halton(2,3) subpixel jitter of the world and first-person projections and a lowered
//              texture mip bias inside the world window (section 11). The spatial upscale is unchanged.
//   SRStage=4  Stage 3 plus the SR inputs prepared at the seam (linear colour, depth twin, motion target with the
//              first-person rule) and dumped by SRStageCapture; the output is still the bilinear pass (section 12).
//   SRStage=5  Stage 4 with the shared RSF SR pipeline (rsf_get_d3d11_sr_api) evaluating at the seam: DLSS, FSR3, FSR4
//              or XeSS as the shared settings select, the reconstruction re-tonemapped into the engine's generic0.
//              The render size follows the selected backend's quality plan. Any refusal draws the bilinear pass.
// Stages 1 to 4 call no SR provider.
#include "sr_jitter.h"
#include "sr_overlay.h"
#include "sr_pass.h"
#include <rescaleframe/game_api.h>
#include <cstddef>
#include <cstdint>
#include <string>

namespace rsf::anomaly::sr {
struct StageOptions {
    uint32_t stage{};               // 0 off, 1 scale-1.0 swap, 2 render-size window, 3 = 2 + jitter/mip bias, 4 = 3 + SR inputs, 5 = 4 + SR
    float scale{0.67f};             // SRScale, clamped (sr_plan.h); stages 2 to 4, and stage 5 while no backend plans
    bool scale_defaulted{true};     // SRScale missing or unparsable
    bool jitter{true};              // SRJitter (stage 3 and 4; stage 5 needs it), default 1
    bool mip_bias{true};            // SRMipBias=off disables the bias change (stages 3 to 5)
    float mip_bias_offset{default_mip_bias_offset};   // SRMipBias, added to log2(render/output), clamped [-2, 1]
    bool capture{};
    // Stage 3 captures SRStageCaptureCount (default 8, at most 32) CONSECUTIVE world frames from SRStageCaptureStart.
    uint32_t capture_start{600}, capture_count{3}, capture_interval{300};
    std::wstring ini;               // where SRScale is re-read after each device reset (vid_restart applies a new value)
    // Stages 4 and 5 (section 12).
    bool exposure_auto{};           // SRExposure=auto (default constant: a 1x1 exposure of 1.0)
    bool first_person_zero{true};   // SRFirstPersonZero (default 1)
    CutRule cut{};                  // SRCutDistance (metres, default 4), SRCutAngle (degrees, default 45)
    std::wstring streamline_dir, fidelityfx_dir, xess_dir;   // SRStreamlineDir / SRFidelityFXDir / SRXeSSDir
    bool require_signature{};       // SRRequireSignature (default 0)
    SrSettings settings{};          // SR, SRBackend, SRQuality, DLSSPreset (shared frontend keys) at startup
    bool world_window() const noexcept { return stage >= 2 && stage <= 5; }
    bool temporal() const noexcept { return stage >= 3 && stage <= 5; }   // jitter and mip bias in the window
    bool sr_inputs() const noexcept { return stage == 4 || stage == 5; }
    bool selected() const noexcept { return stage == 1 || world_window() || capture; }
};
// Stage 3: the jitter published at the last phase_scene_prepare (all zero for scope frames and below Stage 3). The SR
// pass reads current_jitter(); the motion stage reads current_jitter_uv() for its PS b13 rsf_jitter on every draw.
JitterState current_jitter() noexcept;
JitterUv current_jitter_uv() noexcept;
StageOptions read_options(const wchar_t* ini) noexcept;
// SRScale from the INI, clamped. `defaulted` is set when the key is missing or not a number.
float read_scale(const wchar_t* ini, bool* defaulted = nullptr) noexcept;
// SRMipBias from the INI: a decimal offset clamped to [-2, 1] (missing or unparsable: -0.3), or "off" (enabled false).
float read_mip_bias(const wchar_t* ini, bool* enabled = nullptr) noexcept;
// Called by bootstrap before Observe starts. Inert unless options.selected().
void configure(const StageOptions&, rsf_game_log_fn log, void* user) noexcept;
const StageOptions& options() noexcept;
// Stage 2: a new scale for the next world window (clamped). The twins are rebuilt at the next eligible frame.
void set_scale(float) noexcept;
// Called by the native observer after its own and the geometry hooks are installed on the verified image. Verifies
// every guard of the selected stage and installs the hooks, or refuses and leaves the game stock. Returns true when
// not selected.
bool install(std::byte* image, uint32_t image_size) noexcept;
// Disables the hooks, restores an open window and drops the twins.
bool quiesce() noexcept;
// Called by the native observer immediately before the original IRenderDeviceRender::End (before Present).
void frame_end() noexcept;
// Two log lines with the per-window deltas and the totals, for the observer's periodic log. Nothing when inactive.
void report(void (*emit)(void*, const char*), void* user) noexcept;
bool active() noexcept;

// The shared overlay (overlay_service.cpp reaches these through sr_overlay.h, published by configure()).
OverlayChoices overlay_choices() noexcept;
// Any thread: what reached the last presented frame. Zero (no report) unless SRStage=5 is installed.
int overlay_report(rsf_overlay_sr_report* report) noexcept;
// Any thread: shared settings to apply at the next frame boundary (Stage 5).
void request_settings(const SrSettings&) noexcept;
}
