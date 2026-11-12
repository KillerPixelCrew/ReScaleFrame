// SPDX-License-Identifier: GPL-3.0-only
#include "motion_history.h"
#include <algorithm>
#include <utility>

namespace rsf::anomaly::motion {
namespace {
std::size_t index(temporal::ViewId view) noexcept { return view == temporal::ViewId::Scope ? 1 : 0; }
std::size_t index(temporal::ProjectionDomain domain) noexcept
{
    return domain == temporal::ProjectionDomain::FirstPerson ? 1 : 0;
}
}

bool MotionHistory::begin_frame(temporal::ViewId view, std::uint64_t frame, std::uint64_t generation,
    std::uint32_t width, std::uint32_t height)
{
    auto& state = views_[index(view)];
    if (state.current && state.current->frame == frame && state.current->generation == generation &&
        state.current->width == width && state.current->height == height)
        return state.previous.has_value();   // a second phase_scene_begin of the same frame (split scene)
    const bool continuous = state.current && frame > state.current->frame && generation == state.current->generation &&
        width == state.current->width && height == state.current->height;
    state.previous = continuous ? std::move(state.current) : std::nullopt;
    state.current = Frame{frame, generation, width, height};
    return state.previous.has_value();
}

bool MotionHistory::active(temporal::ViewId view) const noexcept { return views_[index(view)].current.has_value(); }

CameraStatus MotionHistory::observe_camera(temporal::ViewId view, temporal::ProjectionDomain domain,
    const CameraSample& camera)
{
    auto& state = views_[index(view)];
    if (!state.current) return CameraStatus::NoFrame;
    auto& slot = state.current->cameras[index(domain)];
    if (state.current->camera_conflict[index(domain)]) return CameraStatus::Conflict;
    if (!slot) { slot = camera; return CameraStatus::Recorded; }
    if (*slot == camera) return CameraStatus::Same;
    state.current->camera_conflict[index(domain)] = true;
    slot.reset();
    ++conflicts_;
    return CameraStatus::Conflict;
}

const CameraSample* MotionHistory::previous_camera(temporal::ViewId view, temporal::ProjectionDomain domain) const noexcept
{
    const auto& state = views_[index(view)];
    if (!state.current || !state.previous) return nullptr;
    const auto& slot = state.previous->cameras[index(domain)];
    return slot ? &*slot : nullptr;
}

bool MotionHistory::retired(temporal::InstanceKey key) const
{
    const auto found = retired_.find({key.owner, key.visual});
    return found != retired_.end() && key.generation <= found->second;
}

const temporal::InstanceSnapshot* MotionHistory::record(temporal::ViewId view, temporal::InstanceKey key,
    const temporal::InstanceSnapshot& snapshot)
{
    auto& state = views_[index(view)];
    if (!state.current || !key.owner || !key.visual || !key.generation || retired(key)) return nullptr;
    const Slot slot{key, snapshot.domain};
    auto [entry, inserted] = state.current->instances.try_emplace(slot, snapshot);
    if (!inserted) {
        if (!entry->second) return nullptr;
        if (*entry->second != snapshot) { entry->second.reset(); ++conflicts_; return nullptr; }
    }
    if (!state.previous) return nullptr;
    const auto old = state.previous->instances.find(slot);
    if (old == state.previous->instances.end() || !old->second) return nullptr;
    const auto& previous = *old->second;
    if (previous.kind != snapshot.kind || previous.palette_layout != snapshot.palette_layout ||
        previous.bones.size() != snapshot.bones.size() || previous.deformation.has_value() != snapshot.deformation.has_value())
        return nullptr;
    return &previous;
}

void MotionHistory::retire(temporal::InstanceKey key)
{
    if (!key.owner || !key.visual || !key.generation) return;
    auto& watermark = retired_[{key.owner, key.visual}];
    watermark = std::max(watermark, key.generation);
    for (auto& view : views_)
        for (auto* frame : {view.current ? &*view.current : nullptr, view.previous ? &*view.previous : nullptr}) {
            if (!frame) continue;
            std::erase_if(frame->instances, [&](const auto& item) {
                return item.first.first.owner == key.owner && item.first.first.visual == key.visual &&
                    item.first.first.generation <= watermark;
            });
        }
}

void MotionHistory::reset_all() { views_ = {}; }
}
