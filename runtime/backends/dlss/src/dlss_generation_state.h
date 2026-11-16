// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/frame_generation.h>
#include <algorithm>
#include <mutex>

namespace rsf {
// CPU copies only. No method calls an SDK, waits for the GPU, or borrows a lock
// through a vendor operation. The revision makes abort win over late publication.
class DlssGenerationState {
public:
    struct Snapshot {
        rsf_fg_status status{};
        rsf_fg_options options{};
        rsf_fg_retirement retirement{sizeof(rsf_fg_retirement)};
        uint64_t revision{}, last_prepare{};
        bool history_valid{}, prepared_enabled{};
    };
private:
    mutable std::mutex guard_;
    Snapshot value_;
public:
    Snapshot read() const { std::lock_guard lock(guard_); return value_; }
    void initialize(const rsf_fg_status& status) {
        std::lock_guard lock(guard_); value_.status = status;
    }
    void configured(const rsf_fg_options& options, uint32_t reflex, uint32_t generated, bool generation_changed) {
        std::lock_guard lock(guard_);
        value_.options = options; value_.status.effective_reflex = reflex;
        if (!generation_changed) return; // Latency-only changes preserve current history/invalidation.
        ++value_.revision;
        value_.status.configured_mode = value_.status.effective_mode = options.mode;
        value_.status.effective_generated_frames = generated;
        value_.status.active = 0; value_.history_valid = value_.prepared_enabled = false;
    }
    bool stage_prepare(uint64_t revision, uint64_t id, uint32_t mode, uint32_t generated) {
        std::lock_guard lock(guard_);
        if (revision != value_.revision || id <= value_.last_prepare) return false;
        value_.status.effective_mode = mode; value_.status.effective_generated_frames = generated;
        value_.status.active = 0; value_.prepared_enabled = false; value_.history_valid = false;
        return true;
    }
    bool complete_prepare(uint64_t revision, uint64_t id, bool enabled) {
        std::lock_guard lock(guard_);
        // A successfully tagged but aborted source is still consumed for ordering.
        value_.last_prepare = std::max(value_.last_prepare, id);
        if (revision != value_.revision) return false;
        value_.history_valid = value_.prepared_enabled = enabled;
        return true;
    }
    void abort() {
        std::lock_guard lock(guard_); ++value_.revision;
        value_.history_valid = value_.prepared_enabled = false; value_.status.active = 0;
    }
    void paced() {
        std::lock_guard lock(guard_);
        if (value_.status.effective_reflex != RSF_REFLEX_OFF) ++value_.status.latency_paced_frames;
    }
    rsf_fg_status polled(uint64_t revision, const rsf_fg_status& observed, uint64_t presented,
        bool sdk_active, const rsf_fg_retirement& retirement) {
        std::lock_guard lock(guard_);
        auto& state = value_.status;
        // Publish only observed fields, preserving newer configuration and cumulative counters.
        state.max_generated_frames = observed.max_generated_frames;
        state.mfg_unlock_status = observed.mfg_unlock_status;
        state.dynamic_supported = observed.dynamic_supported; state.vsync_supported = observed.vsync_supported;
        state.min_dimension = observed.min_dimension; state.vendor_status = observed.vendor_status;
        state.low_latency_available = observed.low_latency_available; state.valid_statistics = observed.valid_statistics;
        state.total_presented += presented;
        state.active = revision == value_.revision && value_.history_valid && value_.prepared_enabled && sdk_active;
        value_.retirement = retirement;
        return state;
    }
};
}
