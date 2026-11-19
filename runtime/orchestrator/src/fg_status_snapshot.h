// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_fg.h>

extern "C" {
void rsf_core_fg_completed_activity(const rsf_fg_status* fresh, const rsf_fg_status* completed,
    uint32_t same_provider, uint32_t eligible, rsf_fg_status* out);
void rsf_core_fg_observe_completed(const rsf_native_fg_status* pending, uint64_t pending_generation,
    uint64_t present_generation, uint64_t source_id, uint32_t success, uint64_t current_generation,
    uint64_t* generation, rsf_native_fg_status* status);
void rsf_core_fg_deactivate_completed(rsf_fg_status* status);
uint32_t rsf_core_fg_completed_matches(uint64_t completed_generation, uint64_t current_generation, uint64_t source_id);
void rsf_core_fg_coherent_status(const rsf_native_fg_status* pending, uint64_t pending_generation,
    const rsf_native_fg_status* completed, uint64_t completed_generation, uint64_t generation,
    uint32_t owner, uint32_t requested, rsf_native_fg_status* out);
}

namespace rsf::fg_status {
inline rsf_fg_status with_completed_activity(const rsf_fg_status& fresh,
    const rsf_fg_status& completed, bool same_provider, bool eligible)
{
    rsf_fg_status out;
    rsf_core_fg_completed_activity(&fresh, &completed, same_provider, eligible, &out);
    return out;
}

struct Completed {
    uint64_t generation = 0;
    rsf_native_fg_status status{};
    void observe(const rsf_native_fg_status& pending, uint64_t pending_generation,
        uint64_t present_generation, uint64_t source_id, bool success, uint64_t current_generation)
    {
        rsf_core_fg_observe_completed(&pending, pending_generation, present_generation,
            source_id, success, current_generation, &generation, &status);
    }
    void deactivate() { rsf_core_fg_deactivate_completed(&status.vendor); }
    bool matches(uint64_t current) const
    {
        return rsf_core_fg_completed_matches(generation, current, status.source_frame_id) != 0;
    }
};

inline rsf_native_fg_status coherent(const rsf_native_fg_status& pending, uint64_t pending_generation,
    const Completed& completed, uint64_t generation, bool owner, bool requested)
{
    rsf_native_fg_status out;
    rsf_core_fg_coherent_status(&pending, pending_generation, &completed.status,
        completed.generation, generation, owner, requested, &out);
    return out;
}
}
