// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/frame_generation.h>
namespace rsf::latency {
extern "C" uint32_t rsf_core_latency_pacing_owner(uint32_t required, uint32_t reflex_available,
    uint32_t antilag_available, uint32_t amd_owner);
// Required SDK pacing wins; otherwise latency is independent of the selected generation provider.
inline uint32_t pacing_owner(uint32_t required, bool reflex_available, bool antilag_available,
    uint32_t amd_owner = RSF_PACING_ANTILAG2) noexcept
{
    return rsf_core_latency_pacing_owner(required, reflex_available, antilag_available, amd_owner);
}
}
