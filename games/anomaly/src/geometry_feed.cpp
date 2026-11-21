// SPDX-License-Identifier: MIT
#include "geometry_feed.h"
#include "geometry_retirement.h"

namespace rsf::anomaly::geometry {

static_assert(sizeof(SnapshotFacts) == 64 && offsetof(SnapshotFacts, world) == 24 &&
    offsetof(SnapshotFacts, view) == 56 && offsetof(SnapshotFacts, draw_kind) == 58);

RetirementQueue::RetirementQueue() : state_(rsf_anomaly_geometry_retirements_create()) {}
RetirementQueue::~RetirementQueue() { rsf_anomaly_geometry_retirements_destroy(state_); }

void RetirementQueue::sink(void* queue, temporal::InstanceKey key) noexcept
{
    if (!queue) return;
    const auto retired = geometry_key(key);
    rsf_anomaly_geometry_enqueue(static_cast<RetirementQueue*>(queue)->state_, &retired);
}

std::size_t RetirementQueue::drain(void (*apply)(void*, temporal::InstanceKey), void* user)
{
    RetirementCall call{apply, user};
    const auto count = rsf_anomaly_geometry_drain(state_, apply ? RetirementCall::visit : nullptr, &call);
    if (call.exception) std::rethrow_exception(call.exception);
    return count;
}

std::uint64_t RetirementQueue::dropped() const noexcept
{
    return rsf_anomaly_geometry_dropped(state_);
}

SnapshotFacts snapshot_facts(const DrawCapture& draw) noexcept
{
    return {geometry_key(draw.key), draw.world ? &*draw.world : nullptr, draw.bones.data(), draw.bones.size(),
        draw.palette_layout, draw.view ? uint8_t(*draw.view) : uint8_t{255},
        draw.domain ? uint8_t(*draw.domain) : uint8_t{255}, uint8_t(draw.kind)};
}

}
