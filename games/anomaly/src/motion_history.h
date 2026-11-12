// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// Draw-time history for the motion MRT: the previous frame's camera per projection domain and the previous frame's
// instance snapshots (world, bone palette) from the geometry feed (geometry_feed.h to_snapshot()).
//
// Why not temporal::TemporalHistory directly: its begin_frame() needs both the world and the first-person camera up
// front, but the first-person projection only exists once r_dsgraph_render_hud has rebuilt it, after the G-buffer
// pass began. Here each camera is the one the engine actually used for the draw (RCache.xforms view/projection),
// recorded at the first draw of its domain. The invariants are the same: only the immediately preceding frame of the
// same view contributes; a reset generation or a size change drops everything; kind, palette layout and bone count
// must match; a same-frame conflict leaves no history; retirement forbids that generation and all older ones.
// Single render-thread owner; retirements arrive through geometry::RetirementQueue.
#include "../temporal/history.h"
#include <array>
#include <cstdint>
#include <map>
#include <optional>

namespace rsf::anomaly::motion {
struct CameraSample {
    temporal::Mat4 view{};        // column-vector convention
    temporal::Mat4 projection{};
    bool operator==(const CameraSample&) const = default;
};
enum class CameraStatus : std::uint8_t { Recorded, Same, Conflict, NoFrame };

class MotionHistory {
public:
    // Starts `frame` of `view`. Returns true when the previous frame of this view stays usable.
    bool begin_frame(temporal::ViewId, std::uint64_t frame, std::uint64_t generation, std::uint32_t width,
        std::uint32_t height);
    bool active(temporal::ViewId) const noexcept;
    CameraStatus observe_camera(temporal::ViewId, temporal::ProjectionDomain, const CameraSample&);
    // Null when the previous frame had no (or a conflicting) camera for that domain.
    const CameraSample* previous_camera(temporal::ViewId, temporal::ProjectionDomain) const noexcept;
    // Records `snapshot` for the active frame and returns the matching previous snapshot, or null.
    const temporal::InstanceSnapshot* record(temporal::ViewId, temporal::InstanceKey, const temporal::InstanceSnapshot&);
    void retire(temporal::InstanceKey);
    void reset_all();
    std::uint64_t conflicts() const noexcept { return conflicts_; }
private:
    using Slot = std::pair<temporal::InstanceKey, temporal::ProjectionDomain>;
    struct Frame {
        std::uint64_t frame{}, generation{};
        std::uint32_t width{}, height{};
        std::array<std::optional<CameraSample>, 2> cameras{};
        std::array<bool, 2> camera_conflict{};
        std::map<Slot, std::optional<temporal::InstanceSnapshot>> instances; // nullopt = same-frame conflict
    };
    struct View {
        std::optional<Frame> current, previous;
    };
    bool retired(temporal::InstanceKey) const;
    std::array<View, 2> views_{};
    std::map<std::pair<std::uint64_t, std::uint64_t>, std::uint64_t> retired_;
    std::uint64_t conflicts_{};
};
}
