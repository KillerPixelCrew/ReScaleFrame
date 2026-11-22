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
SnapshotFacts snapshot_facts(const DrawCapture&) noexcept;

}
