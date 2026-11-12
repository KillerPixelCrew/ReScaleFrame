// SPDX-License-Identifier: GPL-3.0-only
#include "geometry_feed.h"
#include <new>
#include <utility>

namespace rsf::anomaly::geometry {

void RetirementQueue::sink(void* queue, temporal::InstanceKey key) noexcept
{
    if (!queue) return;
    auto& self = *static_cast<RetirementQueue*>(queue);
    try {
        const std::lock_guard lock(self.mutex_);
        try { self.pending_.push_back(key); } catch (...) { ++self.dropped_; }
    } catch (...) {}
}

std::size_t RetirementQueue::drain(temporal::TemporalHistory& history)
{
    std::vector<temporal::InstanceKey> keys;
    {
        const std::lock_guard lock(mutex_);
        keys.swap(pending_);
    }
    for (const auto key : keys) history.retire(key);
    return keys.size();
}

std::uint64_t RetirementQueue::dropped() const noexcept
{
    const std::lock_guard lock(mutex_);
    return dropped_;
}

std::optional<temporal::InstanceSnapshot> to_snapshot(const DrawCapture& draw, FeedStatus& why) noexcept
{
    try {
        if (!draw.view || !draw.domain) { why = FeedStatus::MissingContext; return std::nullopt; }
        // Only the two draw kinds with a verified world+palette contract. SingleBone replaces the world
        // inside the original Render, software skinning has no palette, and trees need the foliage
        // deformation constants. Each stays invalid coverage rather than becoming camera-only motion.
        if (draw.kind != DrawKind::Rigid && draw.kind != DrawKind::HardwareSkinned) {
            why = FeedStatus::UnsupportedKind;
            return std::nullopt;
        }
        if (!draw.world) { why = FeedStatus::MissingWorld; return std::nullopt; }
        temporal::InstanceSnapshot snapshot;
        snapshot.world = *draw.world;
        snapshot.domain = *draw.domain;
        if (draw.kind == DrawKind::HardwareSkinned) {
            if (draw.bones.empty() || draw.palette_layout == 0) { why = FeedStatus::MissingPalette; return std::nullopt; }
            snapshot.kind = temporal::InstanceKind::Skinned;
            snapshot.palette_layout = draw.palette_layout;
            snapshot.bones = draw.bones;
        }
        why = FeedStatus::Captured;
        return snapshot;
    } catch (...) {
        why = FeedStatus::Rejected;
        return std::nullopt;
    }
}

FeedResult feed_current_draw(temporal::TemporalHistory& history, temporal::FrameToken frame, RetirementQueue& queue)
{
    queue.drain(history);
    const auto draw = capture_current_draw();
    if (!draw) return {FeedStatus::NoDraw, std::nullopt};
    if (draw->view && *draw->view != frame.view) return {FeedStatus::WrongView, std::nullopt};
    FeedStatus why{};
    const auto snapshot = to_snapshot(*draw, why);
    if (!snapshot) return {why, std::nullopt};
    const auto stored = history.capture(frame, draw->key, *snapshot);
    const bool ok = stored.status == temporal::CaptureStatus::Captured || stored.status == temporal::CaptureStatus::Repeated;
    return {ok ? FeedStatus::Captured : FeedStatus::Rejected, stored.status};
}

}
