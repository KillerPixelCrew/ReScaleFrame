// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// Per-draw history decision for the core VS b13 buffer. Pure; no engine memory or D3D.
#include "motion_constants.h"
#include "motion_foliage.h"
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
    Foliage,             // foliage family draw the foliage path cannot describe (no layout, not a tree or detail draw)
    FoliageNoConstants,  // the draw's staged constants could not be read, or the buffer did not match the reflection
    FoliageNoPrevious,   // the tree was not drawn last frame, or the detail route was not (permutation or wave set)
    FoliageLodChanged,   // the progressive tree draws a different slide window than last frame
    FoliageXformChanged, // the visual address now carries a different m_xform: a recycled address, not the same tree
    FoliageWorld,        // the detail draw's world matrix is not exactly identity (the transform assumes it)
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

// Foliage. A tree draw is valid when the same visual was drawn with the same xform (and slide window) last frame.
struct TreeDrawInput {
    Family family{};                // bound VS
    bool in_visual{};
    geometry::DrawKind kind{};
    geometry::DrawVerdict verdict{};
    bool context{};
    bool camera_conflict{};
    const CameraSample* previous_camera{};
    bool constants_ok{};
    TreeRecord current{};
    const TreeRecord* previous{};   // FoliageHistory::record_tree
    float previous_jitter_u{}, previous_jitter_v{};
};
struct TreeDecision {
    HistoryReason reason{HistoryReason::NoVisual};
    temporal::Mat4 previous_clip{};   // unjittered previous projection * view
};
TreeDecision decide_tree(const TreeDrawInput&) noexcept;

// A detail draw is valid at draw level when the previous camera and the same route exist; each instance is then
// valid on its own (DetailRemap::items).
struct DetailDrawInput {
    Family family{};                // bound VS
    bool detail_window{};           // inside CDetailManager::Render
    bool context{};
    bool camera_conflict{};
    const CameraSample* previous_camera{};
    bool constants_ok{};
    bool world_readable{};
    bool world_identity{};
    bool route_found{};
    std::uint32_t live_groups{};
    float previous_jitter_u{}, previous_jitter_v{};
};
struct DetailDecision {
    HistoryReason reason{HistoryReason::NoVisual};
    temporal::Mat4 previous_clip{};
};
DetailDecision decide_detail(const DetailDrawInput&) noexcept;
}
