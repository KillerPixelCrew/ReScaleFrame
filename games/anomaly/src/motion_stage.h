// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// Motion MRT for the standard 1.5.3 DX11 executable. Opt-in, rides on Mode=Observe:
//   [Anomaly] Motion=1                 load motion shader variants, bind RTV2 during the G-buffer pass
//   MotionShaderDir=<path>             deployment from tools/anomaly_motion_deploy.py
//                                      (default: <directory of ReScaleFrame.ini>\ReScaleFrame\anomaly-motion)
//   MotionInvalidate=1                 replay occluding draws without a variant as invalid motion (default 1)
//   MotionCapture=1                    dump the motion target and G-buffer through the core frame dump service;
//   MotionCaptureStart=600, MotionCaptureCount=3 (max 16), MotionCaptureInterval=300 (main-view frames), and F10
//                                      (refused while SRStageCapture owns the dump service)
//
// Pipeline (see the headers for each piece):
// * create_shader detour (shader_loader.h): the engine creates, reflects and signature-extracts its stock shader
//   unchanged; for an allowlisted root RSF builds the verified motion variant of that exact permutation and records
//   stock -> variant. Every refusal leaves the stock shader in use.
// * phase_scene_begin/end detours: the G-buffer window. The first open of a frame starts the draw-time history
//   frame (motion_history.h), sizes the RGBA16F target to the bound rt_Position texture (so it follows an SR render-
//   size twin) and clears it to zero.
// * CBackend::constants.flush (the geometry producer's draw-time site, geometry_set_flush_callback): after every
//   native upload, keep RTV2 bound across engine binds (motion_target.h), record the draw's camera and instance
//   snapshot, and for a G-buffer draw whose VS and PS both have linkable variants upload VS/PS b13 and bind the
//   variants. Other occluding draws are marked for the invalidation replay (motion_d3d11.h).
// * ID3D11DeviceContext Draw/DrawIndexed/DrawInstanced/DrawIndexedInstanced detours on the game's immediate
//   context: run the engine's draw, then the invalidation replay if marked, then put the stock shaders back so the
//   engine's shader cache matches the device again.
// Rendering-readiness stays false: this produces a motion target, nothing consumes it yet.
#include <rescaleframe/game_api.h>
#include <cstddef>
#include <cstdint>
#include <string>

namespace rsf::anomaly::motion {
struct MotionOptions {
    bool enabled{};
    std::wstring shader_dir;
    bool invalidate{true};
    bool capture{};
    std::uint32_t capture_start{600}, capture_count{3}, capture_interval{300};
};
MotionOptions read_options(const wchar_t* ini) noexcept;
void configure(const MotionOptions&, rsf_game_log_fn log, void* user) noexcept;
const MotionOptions& options() noexcept;
// After the observer's and geometry hooks on the verified image. True when not enabled. A refusal logs and leaves
// every shader and target stock.
bool install(std::byte* image, std::uint32_t image_size) noexcept;
bool quiesce() noexcept;
bool active() noexcept;
// Log lines for the observer's periodic report. Nothing when inactive.
void report(void (*emit)(void*, const char*), void* user) noexcept;
// Current and previous jitter in normalized viewport UV (Y down) for the core PS b13. Zero until SR jitter exists.
// Foliage draws always receive previous jitter zero (their previous projection is unjittered).
void set_jitter_uv(float current_u, float current_v, float previous_u, float previous_v) noexcept;

struct MotionCounters {
    static constexpr std::size_t families = 6, outcomes = 10, reasons = 11;
    std::uint64_t shaders[families][outcomes]{};      // create_shader results per family (index 0: not allowlisted)
    std::uint64_t frame_refusals{};                    // allowlisted root but the caller frame was not accepted
    std::uint64_t frames{}, windows{}, clears{}, target_created{}, target_failed{}, msaa_frames{};
    std::uint64_t window_draws{}, motion_draws{}, valid_draws{}, invalid_draws{}, reasons_by[reasons]{};
    std::uint64_t unpaired_draws{}, replays{}, replay_skipped_no_depth_write{}, replay_failed{};
    std::uint64_t rebinds{}, already_bound{}, unbinds{}, close_unbinds{}, conflicts{}, binder_faults{};
    std::uint64_t restores{}, stale_restores{}, snapshots{}, camera_conflicts{}, retirements{}, upload_failures{};
    std::uint64_t captures{}, images{}, capture_failed{};
};
MotionCounters counters() noexcept;
}
