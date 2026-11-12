// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// SR Stage 1 for the standard 1.5.3 DX11 executable: the scale-1.0 world-window twin swap with FP16 generic0 /
// generic_temp and the f^-1/f round trip at the S-B seam (docs/research/anomaly-sr-seam.md, Stage 1). No SR provider
// is called. Opt-in only:
//   [Anomaly] SRStage=1          swap generic0/generic_temp to FP16 twins from phase_scene_prepare to the seam
//   [Anomaly] SRStageCapture=1   dump seam and presented frames through the core frame dump service (also with
//                                SRStage=0, where it observes the stock seam without changing anything)
//   SRStageCaptureStart=600, SRStageCaptureCount=3, SRStageCaptureInterval=300 (world frames)
// Both need Mode=Observe: the native observer's prepare() is what verifies the executable.
#include <rescaleframe/game_api.h>
#include <cstddef>
#include <cstdint>

namespace rsf::anomaly::sr {
struct StageOptions {
    uint32_t stage{};               // 0 off, 1 scale-1.0 twin swap
    bool capture{};
    uint32_t capture_start{600}, capture_count{3}, capture_interval{300};
    bool selected() const noexcept { return stage == 1 || capture; }
};
StageOptions read_options(const wchar_t* ini) noexcept;
// Called by bootstrap before Observe starts. Inert unless options.selected().
void configure(const StageOptions&, rsf_game_log_fn log, void* user) noexcept;
const StageOptions& options() noexcept;
// Called by the native observer after its own and the geometry hooks are installed on the verified image. Verifies
// every Stage 1 guard and installs the hooks, or refuses and leaves the game stock. Returns true when not selected.
bool install(std::byte* image, uint32_t image_size) noexcept;
// Disables the hooks, restores an open window and drops the twins.
bool quiesce() noexcept;
// Called by the native observer immediately before the original IRenderDeviceRender::End (before Present).
void frame_end() noexcept;
// Two log lines with the per-window deltas and the totals, for the observer's periodic log. Nothing when inactive.
void report(void (*emit)(void*, const char*), void* user) noexcept;
bool active() noexcept;
}
