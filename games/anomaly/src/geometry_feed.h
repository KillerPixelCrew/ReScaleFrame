// SPDX-License-Identifier: MIT
#pragma once
#include "native_geometry.h"
#include <cstddef>
#include <cstdint>

namespace rsf::anomaly::geometry {

// Borrowed from the guarded draw capture for this call. 255 denotes missing view or domain.
struct SnapshotFacts {
    rsf_anomaly_geometry_key key;
    const temporal::Mat4* world;
    const temporal::Mat4* bones;
    std::size_t bone_count;
    std::uint64_t palette_layout;
    std::uint8_t view, domain, draw_kind;
};
static_assert(sizeof(SnapshotFacts) == 64 && offsetof(SnapshotFacts, world) == 24 &&
    offsetof(SnapshotFacts, view) == 56 && offsetof(SnapshotFacts, draw_kind) == 58);

inline SnapshotFacts snapshot_facts(const DrawCapture& draw) noexcept
{
    return {{draw.key.owner, draw.key.visual, draw.key.generation}, draw.world ? &*draw.world : nullptr,
        draw.bones.data(), draw.bones.size(), draw.palette_layout,
        draw.view ? uint8_t(*draw.view) : uint8_t{255},
        draw.domain ? uint8_t(*draw.domain) : uint8_t{255}, uint8_t(draw.kind)};
}

}
