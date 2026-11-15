// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_fg.h>

namespace rsf::fg_status {
// Capabilities/configuration are current. Activity and cumulative counters describe
// the last completed application Present, never a half-prepared next source.
inline rsf_fg_status with_completed_activity(const rsf_fg_status& fresh,
    const rsf_fg_status& completed, bool same_provider, bool eligible)
{
    auto value = fresh;
    constexpr uint32_t observed = RSF_FG_STAT_TOTAL_PRESENTED | RSF_FG_STAT_GENERATED_PRESENTED |
        RSF_FG_STAT_PRESENT_CALLBACKS | RSF_FG_STAT_ACTIVITY;
    value.valid_statistics = (fresh.valid_statistics & ~observed) |
        (same_provider ? completed.valid_statistics & observed : 0u);
    value.total_presented = same_provider ? completed.total_presented : 0;
    value.generated_presented = same_provider ? completed.generated_presented : 0;
    value.present_callbacks = same_provider ? completed.present_callbacks : 0;
    value.generated_callbacks = same_provider ? completed.generated_callbacks : 0;
    const bool enabled = same_provider && eligible && fresh.supported && fresh.max_generated_frames && fresh.configured_mode != RSF_FG_OFF && !fresh.vendor_status;
    value.active = enabled ? completed.active : 0;
    value.effective_mode = enabled ? completed.effective_mode : RSF_FG_OFF;
    value.effective_generated_frames = enabled ? completed.effective_generated_frames : 0;
    return value;
}
struct Completed {
    uint64_t generation = 0;
    rsf_native_fg_status status{};
    void observe(const rsf_native_fg_status& pending, uint64_t pending_generation,
        uint64_t present_generation, uint64_t source_id, bool success, uint64_t current_generation) {
        if (!success || !source_id || pending.source_frame_id != source_id ||
            pending_generation != current_generation || present_generation != current_generation) {
            *this = {}; return;
        }
        generation = current_generation; status = pending;
    }
    void deactivate() {
        status.vendor.active = 0; status.vendor.effective_mode = RSF_FG_OFF;
        status.vendor.effective_generated_frames = 0;
    }
    bool matches(uint64_t current) const {
        return generation && generation == current && status.source_frame_id;
    }
};
inline rsf_native_fg_status coherent(const rsf_native_fg_status& pending, uint64_t pending_generation,
    const Completed& completed, uint64_t generation, bool owner, bool requested)
{
    const bool same = owner && completed.matches(generation);
    const bool eligible = owner && requested && pending_generation == generation &&
        pending.reason == RSF_NATIVE_FG_REASON_NONE && pending.last_result == RSF_BACKEND_OK;
    auto value = same && eligible ? completed.status : pending;
    value.vendor = with_completed_activity(pending.vendor, completed.status.vendor, same, eligible);
    if (pending_generation != generation && !same) {
        value.source_frame_id = 0; value.input_qpc = value.source_qpc = value.source_qpc_frequency = 0;
    }
    return value;
}
}
