// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// Per-draw history decision for the core VS b13 buffer. Pure; no engine memory or D3D.
#include "motion_constants.h"
#include "motion_history.h"
#include "native_geometry.h"
#include "shader_loader.h"
#include <cstdint>
#include <span>

namespace rsf::anomaly::motion {
enum class HistoryReason : std::uint8_t {
    Valid,
    NoVisual,            // the draw is outside any hooked visual (details, LODs, full-screen passes)
    Unidentified,        // the geometry verdict is neither identified nor static world
    NoContext,           // the draw has no view or projection domain
    UnsupportedKind,     // single-bone, software-skinned or tree draw: no verified previous-state schema
    KindMismatch,        // the bound VS permutation does not fit the draw kind
    NoPreviousCamera,    // no camera for this view/domain in the previous frame (first frame, reset, cut)
    CameraConflict,      // this frame's camera for the domain changed mid-pass
    NoPreviousInstance,  // no matching previous snapshot (new, retired, re-generated, culled last frame)
    TooManyBones,        // more than 78 palette bones
    Foliage,             // foliage family: RSF supplies no tree/detail history yet
    Count
};
const char* history_reason_name(HistoryReason) noexcept;

struct CoreDrawInput {
    Family family{};                // CoreStatic or CoreModel (bound VS)
    int skin{-2};                   // -1 SKIN_NONE, 0..4
    bool in_visual{};
    geometry::DrawVerdict verdict{};
    geometry::DrawKind kind{};
    bool context{};                 // view and domain known
    bool camera_conflict{};
    const CameraSample* previous_camera{};
    const temporal::InstanceSnapshot* previous{};   // from MotionHistory::record for an identified draw
};
struct CoreDecision {
    HistoryReason reason{HistoryReason::NoVisual};
    CoreHistory flags{};
    temporal::Mat4 previous_wvp{};
    std::span<const temporal::Mat4> bones;   // borrowed from `previous`
};
CoreDecision decide_core(const CoreDrawInput&) noexcept;
}
