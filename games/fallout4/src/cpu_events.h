// SPDX-License-Identifier: MIT
#pragma once
#include "cpu_events_platform.h"

namespace rsf::fallout4 {
// The CPU event owner is Rust (cpu_events.rs). Identity is borrowed during the guarded Swap.
// A proved PlayerControls consumer may adopt a reserved engine-buffer poll source before Swap;
// a selected mode1 worker can hand one consumed successor through its native post-Swap join to
// the next ordinary world. Otherwise Swap opens a rendered source. No simulation/GPU stage is
// implied. Native RAWINPUT, selected removed window messages and delivered gamepad batches are
// separate unframed receipts. They never assert a physical sample. The executable-local message
// thunk and its saved continuation stay pinned after stop, including when a later IAT owner
// retains our address.
struct CpuIdentity {
    uint64_t session{}, source{}, submission{}, began_qpc{}, frequency{};
};
static_assert(sizeof(CpuIdentity) == sizeof(rsf_fallout4_cpu_identity));

inline rsf_fallout4_cpu_identity cpu_identity_abi(const CpuIdentity& value) noexcept {
    return {value.session, value.source, value.submission, value.began_qpc, value.frequency};
}
inline CpuIdentity current_cpu_identity() noexcept {
    rsf_fallout4_cpu_identity value{};
    rsf_fallout4_cpu_current_identity(&value);
    return {value.session, value.source, value.submission, value.began_qpc, value.frequency};
}
// Called once by the primary render producer inside the active Swap scope.
// Supplies a render-pass association, never an ExecuteCommandLists fact.
inline bool begin_cpu_render() noexcept { return rsf_fallout4_cpu_begin_render(); }
// The caller separately proves its WorldBegin parent. Reserved sources also
// require strict pre-bind poll/raw ownership. A sealed rendered predecessor
// survives only its explicitly mapped successor; unmatched changes refuse.
// A rendered fallback does not assert input ownership or classify gameplay.
inline bool cpu_source_is_current(const CpuIdentity& identity) noexcept {
    const auto value = cpu_identity_abi(identity);
    return rsf_fallout4_cpu_source_is_current(&value);
}
// Existing render association only: later input receipts do not erase rendered
// resources. This does not certify sampled input, latency markers or FG eligibility.
inline bool render_source_is_current(const CpuIdentity& identity) noexcept {
    const auto value = cpu_identity_abi(identity);
    return rsf_fallout4_cpu_render_source_is_current(&value);
}
// Called only by the existing guarded native Present detour after verified
// owner/screen checks and SUBMISSION completion. These are native phase facts.
inline bool begin_cpu_present_phase(const CpuIdentity& identity, uint64_t native_frame, uint64_t window) noexcept {
    const auto value = cpu_identity_abi(identity);
    return rsf_fallout4_cpu_begin_present_phase(&value, native_frame, window);
}
inline void end_cpu_present_phase(const CpuIdentity& identity) noexcept {
    const auto value = cpu_identity_abi(identity);
    rsf_fallout4_cpu_end_present_phase(&value);
}
// The native leaves of the Rust owner, published with the GameSDK table.
const rsf_fallout4_cpu_platform* cpu_event_platform() noexcept;
}
