// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_api.h>

namespace rsf::fallout4 {
// Identity is borrowed during the guarded Swap. A proved PlayerControls
// consumer may adopt a reserved engine-buffer poll source before Swap;
// otherwise Swap opens a rendered source. No simulation/GPU stage is implied.
// Native keyboard/mouse RAWINPUT receipts are reported separately, unframed.
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
// require unchanged poll/raw-input generation and eligible native state.
// A rendered fallback does not assert input ownership or classify gameplay.
bool cpu_source_is_current(const CpuIdentity&) noexcept;
}
