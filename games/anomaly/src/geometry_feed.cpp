// SPDX-License-Identifier: MIT
#include "geometry_feed.h"
#include "geometry_retirement.h"

namespace rsf::anomaly::geometry {

static_assert(sizeof(SnapshotFacts) == 64 && offsetof(SnapshotFacts, world) == 24 &&
    offsetof(SnapshotFacts, view) == 56 && offsetof(SnapshotFacts, draw_kind) == 58);

SnapshotFacts snapshot_facts(const DrawCapture& draw) noexcept
{
    return {geometry_key(draw.key), draw.world ? &*draw.world : nullptr, draw.bones.data(), draw.bones.size(),
        draw.palette_layout, draw.view ? uint8_t(*draw.view) : uint8_t{255},
        draw.domain ? uint8_t(*draw.domain) : uint8_t{255}, uint8_t(draw.kind)};
}

}
