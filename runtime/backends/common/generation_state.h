// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/frame_generation.h>
#include <algorithm>
#include <mutex>

namespace rsf {
// Shared CPU validity/copying only. Providers retain SDK result interpretation,
// callback meanings, DXGI accounting and queue retirement. No method calls out.
class GenerationState {
public:
    struct Snapshot {
        rsf_fg_status status{};
        rsf_fg_options options{};
        uint64_t revision{}, last_prepare{};
        bool history_valid{}, prepared_enabled{};
    };
    struct Observation {
        int32_t vendor_status{};
        uint32_t valid_statistics{};
        uint64_t total_presented_delta{}, generated_callbacks{}, present_callbacks{};
        bool replace_statistics{}, replace_callbacks{}, active{};
    };
private:
    mutable std::mutex guard_;
    Snapshot value_;
public:
    Snapshot read() const { std::lock_guard lock(guard_); return value_; }
    void initialize(const rsf_fg_status& status) { std::lock_guard lock(guard_); value_.status=status; }
    void configured(const rsf_fg_options& options, uint32_t mode, uint32_t generated, uint32_t reflex, bool reset) {
        std::lock_guard lock(guard_);
        value_.options=options; value_.status.configured_mode=options.mode; value_.status.effective_mode=mode;
        value_.status.effective_generated_frames=generated; value_.status.effective_reflex=reflex;
        if (reset) {
            ++value_.revision; value_.history_valid=value_.prepared_enabled=false; value_.status.active=0;
        }
    }
    bool reset_history(uint64_t revision) {
        std::lock_guard lock(guard_);
        if (revision!=value_.revision) return false;
        value_.history_valid=value_.prepared_enabled=false; value_.status.active=0; return true;
    }
    bool stage_prepare(uint64_t revision, uint64_t id, uint32_t mode, uint32_t generated) {
        std::lock_guard lock(guard_);
        if (revision!=value_.revision || id<=value_.last_prepare) return false;
        value_.status.effective_mode=mode; value_.status.effective_generated_frames=generated;
        value_.history_valid=value_.prepared_enabled=false; value_.status.active=0; return true;
    }
    bool complete_prepare(uint64_t revision, uint64_t id, bool enabled, uint32_t mode, uint32_t generated) {
        std::lock_guard lock(guard_);
        value_.last_prepare=std::max(value_.last_prepare,id);
        if (revision!=value_.revision) return false;
        value_.history_valid=value_.prepared_enabled=enabled;
        value_.status.effective_mode=mode; value_.status.effective_generated_frames=generated; value_.status.active=0;
        return true;
    }
    void abort() {
        std::lock_guard lock(guard_); ++value_.revision;
        value_.history_valid=value_.prepared_enabled=false; value_.status.active=0;
    }
    void paced() { std::lock_guard lock(guard_); ++value_.status.latency_paced_frames; }
    rsf_fg_status observed(uint64_t revision, const Observation& observation) {
        std::lock_guard lock(guard_); auto& state=value_.status;
        state.vendor_status=observation.vendor_status;
        if (observation.replace_statistics) state.valid_statistics=observation.valid_statistics;
        else state.valid_statistics|=observation.valid_statistics;
        if (observation.replace_callbacks) {
            state.generated_callbacks=observation.generated_callbacks; state.present_callbacks=observation.present_callbacks;
        }
        state.total_presented+=observation.total_presented_delta;
        state.active=revision==value_.revision && value_.history_valid && value_.prepared_enabled && observation.active;
        return state;
    }
};
}
