// SPDX-License-Identifier: GPL-3.0-only
#include "motion_policy.h"

namespace rsf::anomaly::motion {
const char* history_reason_name(HistoryReason reason) noexcept
{
    static constexpr const char* names[] = {"valid", "no_visual", "unidentified", "no_context", "unsupported_kind",
        "kind_mismatch", "no_previous_camera", "camera_conflict", "no_previous_instance", "too_many_bones", "foliage"};
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
    out.bones = bones;
    out.flags = {true, palette, in.family == Family::CoreModel && in.skin == -1};
    out.reason = HistoryReason::Valid;
    return out;
}
}
