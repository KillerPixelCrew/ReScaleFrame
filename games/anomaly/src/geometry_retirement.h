// SPDX-License-Identifier: MIT
#pragma once
#include "geometry_registry_platform.h"
#include "temporal_types.h"

namespace rsf::anomaly::geometry {
inline rsf_anomaly_geometry_key geometry_key(temporal::InstanceKey key) noexcept {
    return {key.owner, key.visual, key.generation};
}
}
