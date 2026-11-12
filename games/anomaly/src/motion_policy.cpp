// SPDX-License-Identifier: GPL-3.0-only
#include "motion_policy.h"
#include <cmath>
#include <cstring>
#include <limits>
#include <iterator>

namespace rsf::anomaly::motion {
const char* history_reason_name(HistoryReason reason) noexcept
{
    static constexpr const char* names[] = {"valid", "no_visual", "unidentified", "no_context", "unsupported_kind",
        "kind_mismatch", "no_previous_camera", "camera_conflict", "no_previous_instance", "too_many_bones", "foliage",
        "foliage_no_constants", "foliage_no_previous", "foliage_lod_changed", "foliage_xform_changed", "foliage_world"};
    const auto i = static_cast<std::size_t>(reason);
    return i < std::size(names) ? names[i] : "unknown";
}

CoreDecision decide_core(const CoreDrawInput& in) noexcept
{
    CoreDecision out;
    using geometry::DrawKind;
    using geometry::DrawVerdict;
    if (!in.in_visual) { out.reason = HistoryReason::NoVisual; return out; }
    if (in.verdict != DrawVerdict::Identified && in.verdict != DrawVerdict::IgnoredStaticWorld) {
        out.reason = HistoryReason::Unidentified;
        return out;
    }
    if (!in.context) { out.reason = HistoryReason::NoContext; return out; }
    const bool rigid_permutation = in.family == Family::CoreStatic || (in.family == Family::CoreModel && in.skin == -1);
    const bool skinned_permutation = in.family == Family::CoreModel && in.skin >= 1 && in.skin <= 4;
    // SKIN_0 (CSkeletonX RM_SINGLE): no palette; the engine composed the bone into the draw's world matrix.
    const bool single_bone_permutation = in.family == Family::CoreModel && in.skin == 0;
    temporal::Mat4 world = temporal::Mat4::identity();
    std::span<const temporal::Mat4> bones;
    bool palette = false;
    if (in.verdict == DrawVerdict::IgnoredStaticWorld) {
        // Unregistered rigid visual whose live world is exactly Fidentity: level geometry, previous world identity.
        if (!rigid_permutation) { out.reason = HistoryReason::KindMismatch; return out; }
    } else if (in.kind == DrawKind::Rigid) {
        if (!rigid_permutation) { out.reason = HistoryReason::KindMismatch; return out; }
    } else if (in.kind == DrawKind::HardwareSkinned) {
        if (!skinned_permutation) { out.reason = HistoryReason::KindMismatch; return out; }
    } else if (in.kind == DrawKind::SingleBone) {
        // Needs the identified snapshot of the previous bone-composed world (geometry_feed to_snapshot); an
        // unregistered skeleton is never static world (classify_draw), so it has no identity-world shortcut.
        if (!single_bone_permutation || in.verdict != DrawVerdict::Identified) { out.reason = HistoryReason::KindMismatch; return out; }
    } else {
        out.reason = HistoryReason::UnsupportedKind;
        return out;
    }
    if (in.camera_conflict) { out.reason = HistoryReason::CameraConflict; return out; }
    if (!in.previous_camera) { out.reason = HistoryReason::NoPreviousCamera; return out; }
    if (in.verdict == DrawVerdict::Identified) {
        if (!in.previous) { out.reason = HistoryReason::NoPreviousInstance; return out; }
        world = in.previous->world;
        if (in.kind == DrawKind::HardwareSkinned) {
            if (in.previous->bones.empty()) { out.reason = HistoryReason::NoPreviousInstance; return out; }
            if (in.previous->bones.size() > core_max_bones) { out.reason = HistoryReason::TooManyBones; return out; }
            bones = in.previous->bones;
            palette = true;
        }
    }
    out.previous_wvp = compose(compose(in.previous_camera->projection, in.previous_camera->view), world);
    out.previous_wv = compose(in.previous_camera->view, world);
    out.previous_projection = in.previous_camera->projection;
    out.bones = bones;
    out.flags = {true, palette, in.family == Family::CoreModel && in.skin == -1};
    out.reason = HistoryReason::Valid;
    return out;
}

std::size_t distance_band(double metres) noexcept
{
    if (!std::isfinite(metres) || metres < 0) return distance_bands - 1;
    if (metres < 20) return 0;
    if (metres < 50) return 1;
    if (metres < 100) return 2;
    if (metres < 150) return 3;
    return 4;
}

const char* distance_band_name(std::size_t band) noexcept
{
    static constexpr const char* names[] = {"0-20m", "20-50m", "50-100m", "100-150m", "150m+", "unknown"};
    return band < std::size(names) ? names[band] : "unknown";
}

double camera_distance(const temporal::Mat4& view, const temporal::Mat4& world) noexcept
{
    // view = [R t; 0 1]: camera position = -R^T t.
    const auto& v = view.values;
    if (v[12] != 0 || v[13] != 0 || v[14] != 0 || v[15] != 1) return std::numeric_limits<double>::quiet_NaN();
    double camera[3]{};
    for (int c = 0; c < 3; ++c) camera[c] = -(v[0 * 4 + c] * v[3] + v[1 * 4 + c] * v[7] + v[2 * 4 + c] * v[11]);
    const double dx = world.values[3] - camera[0], dy = world.values[7] - camera[1], dz = world.values[11] - camera[2];
    return std::sqrt(dx * dx + dy * dy + dz * dz);
}

ModelElement model_element(std::string_view root) noexcept
{
    if (!root.starts_with("deffer_model")) return ModelElement::Unknown;
    if (root.find("(tess") != std::string_view::npos) return ModelElement::Tessellated;
    if (root.find("-hq") != std::string_view::npos) return ModelElement::High;
    return ModelElement::Low;
}

bool stale_palette(const temporal::InstanceSnapshot& previous, const temporal::InstanceSnapshot& current, double min_move) noexcept
{
    if (current.bones.empty() || previous.bones.size() != current.bones.size()) return false;
    for (std::size_t i = 0; i < current.bones.size(); ++i)
        if (std::memcmp(previous.bones[i].values.data(), current.bones[i].values.data(), sizeof(current.bones[i].values)) != 0)
            return false;
    const double dx = current.world.values[3] - previous.world.values[3], dy = current.world.values[7] - previous.world.values[7],
                 dz = current.world.values[11] - previous.world.values[11];
    return dx * dx + dy * dy + dz * dz > min_move * min_move;
}

namespace {
temporal::Mat4 foliage_previous_clip(const CameraSample& camera, float jitter_u, float jitter_v) noexcept
{
    return compose(remove_jitter(camera.projection, jitter_u, jitter_v), camera.view);
}
}

TreeDecision decide_tree(const TreeDrawInput& in) noexcept
{
    TreeDecision out;
    using geometry::DrawKind;
    using geometry::DrawVerdict;
    if (in.family != Family::FoliageTree) { out.reason = HistoryReason::KindMismatch; return out; }
    if (!in.in_visual || in.kind != DrawKind::Tree) { out.reason = HistoryReason::NoVisual; return out; }
    if (in.verdict != DrawVerdict::IdentifiedFoliage && in.verdict != DrawVerdict::Identified) {
        out.reason = HistoryReason::Unidentified;
        return out;
    }
    if (!in.context) { out.reason = HistoryReason::NoContext; return out; }
    if (!in.constants_ok) { out.reason = HistoryReason::FoliageNoConstants; return out; }
    if (in.camera_conflict) { out.reason = HistoryReason::CameraConflict; return out; }
    if (!in.previous_camera) { out.reason = HistoryReason::NoPreviousCamera; return out; }
    if (!in.previous) { out.reason = HistoryReason::FoliageNoPrevious; return out; }
    // A level tree's xform never changes, so a different one means this address now names another tree.
    if (std::memcmp(in.previous->xform, in.current.xform, sizeof(in.current.xform)) != 0) {
        out.reason = HistoryReason::FoliageXformChanged;
        return out;
    }
    if (in.previous->lod != in.current.lod) { out.reason = HistoryReason::FoliageLodChanged; return out; }
    out.previous_clip = foliage_previous_clip(*in.previous_camera, in.previous_jitter_u, in.previous_jitter_v);
    out.reason = HistoryReason::Valid;
    return out;
}

DetailDecision decide_detail(const DetailDrawInput& in) noexcept
{
    DetailDecision out;
    if (in.family != Family::FoliageDetail) { out.reason = HistoryReason::KindMismatch; return out; }
    if (!in.detail_window) { out.reason = HistoryReason::NoVisual; return out; }
    if (!in.context) { out.reason = HistoryReason::NoContext; return out; }
    if (!in.constants_ok || in.live_groups == 0) { out.reason = HistoryReason::FoliageNoConstants; return out; }
    // CDetailManager::Render sets Fidentity as the world matrix; the transform acts on world-space positions with the
    // previous view-projection, which is only equal to the original m_WVP while that holds.
    if (!in.world_readable || !in.world_identity) { out.reason = HistoryReason::FoliageWorld; return out; }
    if (in.camera_conflict) { out.reason = HistoryReason::CameraConflict; return out; }
    if (!in.previous_camera) { out.reason = HistoryReason::NoPreviousCamera; return out; }
    if (!in.route_found) { out.reason = HistoryReason::FoliageNoPrevious; return out; }
    out.previous_clip = foliage_previous_clip(*in.previous_camera, in.previous_jitter_u, in.previous_jitter_v);
    out.reason = HistoryReason::Valid;
    return out;
}
}
