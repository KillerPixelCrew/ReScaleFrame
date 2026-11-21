// SPDX-License-Identifier: MIT
#pragma once
#include <cstdint>
namespace rsf::diagnostics {
// Graphics-owner state. A busy/manual request never consumes the failure budget.
struct FailureCapturePolicy {
    uint32_t captured{};
    bool claim(bool enabled, bool request_pending, int32_t result) noexcept {
        if (!enabled || request_pending || result==0 || captured>=3) return false;
        ++captured;
        return true;
    }
};
// Select four distinct ready sources, preserving existing manual/active captures.
struct SequenceCapturePolicy {
    uint32_t remaining{};
    uint64_t session{}, source{};
    void request() noexcept { remaining=4; session=source=0; }
    bool claim(uint64_t next_session, uint64_t next_source, bool ready, bool request_pending) noexcept {
        if (!remaining || !ready || request_pending || !next_session || !next_source ||
            (session==next_session && source==next_source)) return false;
        session=next_session; source=next_source; --remaining;
        return true;
    }
};
}
