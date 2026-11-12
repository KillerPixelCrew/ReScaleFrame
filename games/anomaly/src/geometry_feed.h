// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "native_geometry.h"
#include <cstddef>
#include <cstdint>
#include <mutex>
#include <optional>
#include <vector>

namespace rsf::anomaly::geometry {

// Hands lifetime retirements from the engine lifecycle thread to the render-thread TemporalHistory owner.
// TemporalHistory has a single owner, so the native sink must never call it directly.
class RetirementQueue {
public:
    // GeometrySink::retired target. Thread-safe, noexcept; an allocation failure is counted, not thrown.
    static void sink(void* queue, temporal::InstanceKey key) noexcept;
    // Render thread only. Applies every queued retirement to the history that owns the frame.
    std::size_t drain(temporal::TemporalHistory& history);
    // Same, for another render-thread history owner (the motion stage's draw-time history).
    std::size_t drain(void (*apply)(void* user, temporal::InstanceKey), void* user);
    // Retirements that could not be queued. Stale snapshots stay harmless because the new generation
    // never equals an old key and the registry refuses retired draws, but the count is evidence.
    std::uint64_t dropped() const noexcept;
private:
    mutable std::mutex mutex_;
    std::vector<temporal::InstanceKey> pending_;
    std::uint64_t dropped_{};
};

enum class FeedStatus : std::uint8_t {
    NoDraw,           // No active, still-live identity on this thread.
    MissingContext,   // The draw has no view/projection domain; nothing may be guessed.
    WrongView,        // The draw belongs to a different view than the frame token.
    MissingWorld,     // The actual-draw world matrix was unreadable or not an affine Fmatrix.
    UnsupportedKind,  // Single-bone, software-skinned and tree draws have no verified snapshot schema.
    MissingPalette,   // A hardware-skinned draw whose bone palette could not be copied.
    Rejected,         // TemporalHistory refused the snapshot; see FeedResult::history.
    Captured          // Stored (or an identical repeat of the stored) snapshot.
};
struct FeedResult {
    FeedStatus status{FeedStatus::NoDraw};
    std::optional<temporal::CaptureStatus> history;
};

// Pure mapping: no engine memory is read. Returns nullopt with the reason in `why`.
std::optional<temporal::InstanceSnapshot> to_snapshot(const DrawCapture&, FeedStatus& why) noexcept;
// Render thread, inside the actual draw of the current visual. Drains retirements first, then copies
// the current draw's world/palette and records it for `frame`.
FeedResult feed_current_draw(temporal::TemporalHistory&, temporal::FrameToken frame, RetirementQueue&);
// Same mapping for a capture the draw-time hook already copied (DrawSink::draw), on the history owner's thread.
FeedResult feed_draw(temporal::TemporalHistory&, temporal::FrameToken frame, RetirementQueue&, const DrawCapture&);

}
