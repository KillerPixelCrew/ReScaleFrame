// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/frame_generation.h>
namespace rsf::latency {
// Required SDK pacing wins; otherwise latency is independent of the selected generation provider.
constexpr uint32_t pacing_owner(uint32_t required, bool reflex_available, bool antilag_available) noexcept {
    if (required == RSF_PACING_XELL || required == RSF_PACING_REFLEX) return required;
    if (reflex_available) return RSF_PACING_REFLEX;
    return antilag_available ? RSF_PACING_ANTILAG2 : RSF_PACING_NONE;
}
}
