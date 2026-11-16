// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_api.h>

namespace rsf::fallout4 {
// Identity is borrowed during the guarded Swap. A proved PlayerControls
// consumer may adopt a reserved engine-buffer poll source before Swap;
// a selected mode1 worker can hand one consumed successor through its native
// post-Swap join to the next ordinary world. Otherwise Swap opens a rendered
// source. No simulation/GPU stage is implied.
// Native RAWINPUT, selected removed window messages and delivered gamepad
// batches are separate unframed receipts. They never assert a physical sample.
// The executable-local message thunk and its saved continuation stay pinned
// after stop, including when a later IAT owner retains our address.
struct CpuIdentity {
    uint64_t session{}, source{}, submission{}, began_qpc{}, frequency{};
};
bool prepare_cpu_events(const rsf_game_host_services&);
bool start_cpu_events();
void quiesce_cpu_events();
bool stop_cpu_events();
bool cpu_events_busy();
CpuIdentity current_cpu_identity() noexcept;
// Called once by the primary render producer inside the active Swap scope.
// Supplies a render-pass association, never an ExecuteCommandLists fact.
bool begin_cpu_render() noexcept;
// The caller separately proves its WorldBegin parent. Reserved sources also
// require strict pre-bind poll/raw ownership. A sealed rendered predecessor
// survives only its explicitly mapped successor; unmatched changes refuse.
// A rendered fallback does not assert input ownership or classify gameplay.
bool cpu_source_is_current(const CpuIdentity&) noexcept;
// Existing render association only: later input receipts do not erase rendered
// resources. This does not certify sampled input, latency markers or FG eligibility.
bool render_source_is_current(const CpuIdentity&) noexcept;
// Called only by the existing guarded native Present detour after verified
// owner/screen checks and SUBMISSION completion. These are native phase facts.
bool begin_cpu_present_phase(const CpuIdentity&, uint64_t native_frame, uint64_t window) noexcept;
void end_cpu_present_phase(const CpuIdentity&) noexcept;
}
