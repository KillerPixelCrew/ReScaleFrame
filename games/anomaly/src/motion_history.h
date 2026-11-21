// SPDX-License-Identifier: MIT
#pragma once
// Draw-time history for the motion MRT: the previous frame's camera per projection domain and the previous frame's
// instance snapshots (world, bone palette) from the guarded geometry capture.
//
// Why not temporal::TemporalHistory directly: its begin_frame() needs both the world and the first-person camera up
// front, but the first-person projection only exists once r_dsgraph_render_hud has rebuilt it, after the G-buffer
// pass began. Here each camera is the one the engine actually used for the draw (RCache.xforms view/projection),
// recorded at the first draw of its domain. Only the last recorded frame of the same view contributes. A reset
// generation or a size change drops the frame data. Kind, palette layout and bone count
// must match; a same-frame conflict leaves no history; retirement forbids that generation and all older ones.
// Single render-thread owner; retirements arrive through geometry::RetirementQueue.
#include "../temporal/history.h"
#include <cstdint>
#include <span>

namespace rsf::anomaly::motion {
struct CameraSample {
    temporal::Mat4 view{};        // column-vector convention
    temporal::Mat4 projection{};
    bool operator==(const CameraSample&) const = default;
};
enum class CameraStatus : std::uint8_t { Recorded, Same, Conflict, NoFrame };

struct PaletteView {
    const temporal::Mat4* data{};
    std::size_t count{};
    std::span<const temporal::Mat4> span() const noexcept { return {data, count}; }
};
struct DrawSnapshotView {
    const temporal::Mat4* world{};
    PaletteView bones{};
    explicit operator bool() const noexcept { return world != nullptr; }
};
inline DrawSnapshotView draw_snapshot_view(const temporal::InstanceSnapshot& snapshot) noexcept
{
    return {&snapshot.world, {snapshot.bones.data(), snapshot.bones.size()}};
}

struct MotionHistoryState;
class MotionHistory {
public:
    MotionHistory() = default;
    ~MotionHistory();
    MotionHistory(const MotionHistory&) = delete;
    MotionHistory& operator=(const MotionHistory&) = delete;

    // Starts `frame` of `view`. Returns true when the previous frame of this view stays usable.
    bool begin_frame(temporal::ViewId, std::uint64_t frame, std::uint64_t generation, std::uint32_t width,
        std::uint32_t height);
    bool active(temporal::ViewId) const noexcept;
    CameraStatus observe_camera(temporal::ViewId, temporal::ProjectionDomain, const CameraSample&);
    // Borrows the previous camera for this draw, until the next history mutation.
    const CameraSample* previous_camera(temporal::ViewId, temporal::ProjectionDomain) const noexcept;
    // The returned view borrows Rust-owned data for this draw. Consume before frame transition, retirement or reset.
    DrawSnapshotView record(temporal::ViewId, temporal::InstanceKey, const temporal::InstanceSnapshot&);
    void retire(temporal::InstanceKey);
    void reset_all();
    std::uint64_t conflicts() const noexcept;
private:
    MotionHistoryState* state_{};
};
}
