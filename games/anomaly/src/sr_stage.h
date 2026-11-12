// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// SR stages behind [Anomaly] SRStage (docs/research/anomaly-sr-seam.md sections 9 and 10). Opt-in, Observe mode only.
//   SRStage=1  generic0/generic_temp FP16 twins at output size, f^-1/f round trip at the seam.
//   SRStage=2  every world target at render size (SRScale, default 0.67, clamped to [0.5, 1.0]), Device size and
//              HW.pBaseZB swapped for the world window, bilinear upscale in linear space at the seam.
// No SR provider is called by either stage.
#include <rescaleframe/game_api.h>
#include <cstddef>
#include <cstdint>
#include <string>

namespace rsf::anomaly::sr {
struct StageOptions {
    uint32_t stage{};               // 0 off, 1 scale-1.0 twin swap, 2 render-size world window
    float scale{0.67f};             // SRScale, clamped (sr_plan.h); used by stage 2
    bool scale_defaulted{true};     // SRScale missing or unparsable
    bool capture{};
    uint32_t capture_start{600}, capture_count{3}, capture_interval{300};
    std::wstring ini;               // where SRScale is re-read after each device reset (vid_restart applies a new value)
    bool selected() const noexcept { return stage == 1 || stage == 2 || capture; }
};
StageOptions read_options(const wchar_t* ini) noexcept;
// SRScale from the INI, clamped. `defaulted` is set when the key is missing or not a number.
float read_scale(const wchar_t* ini, bool* defaulted = nullptr) noexcept;
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
}
