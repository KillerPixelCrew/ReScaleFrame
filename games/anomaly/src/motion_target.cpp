// SPDX-License-Identifier: GPL-3.0-only
#include "motion_target.h"

namespace rsf::anomaly::motion {
BindAction MotionBinder::on_draw(std::byte* backend, bool motion_wanted, const void* position_rtv, void* motion_rtv) noexcept
{
    bound_ = false;
    if (!backend || !motion_rtv) return BindAction::None;
    void* targets[4]{};
    void* depth{};
    if (!host_.read(backend + layout_.rt_offset, targets, sizeof(targets)) ||
        !host_.read(backend + layout_.zb_offset, &depth, sizeof(depth))) return BindAction::Fault;
    const bool gbuffer = motion_wanted && position_rtv && targets[0] == position_rtv;
    if (gbuffer) {
        if (targets[2] == motion_rtv) { bound_ = true; return BindAction::AlreadyBound; }
        if (targets[2]) return BindAction::Conflict;
        targets[2] = motion_rtv;
        const bool clean = false;
        if (!host_.write(backend + layout_.rt_offset + 2 * sizeof(void*), &motion_rtv, sizeof(motion_rtv)) ||
            !host_.write(backend + layout_.changed_offset, &clean, sizeof(clean))) return BindAction::Fault;
        host_.set_targets(targets, depth);
        bound_ = true;
        return BindAction::Bound;
    }
    if (targets[2] != motion_rtv) return BindAction::None;
    void* const none{};
    const bool clean = false;
    targets[2] = nullptr;
    if (!host_.write(backend + layout_.rt_offset + 2 * sizeof(void*), &none, sizeof(none)) ||
        !host_.write(backend + layout_.changed_offset, &clean, sizeof(clean))) return BindAction::Fault;
    host_.set_targets(targets, depth);
    return BindAction::Unbound;
}

BindAction MotionBinder::close(std::byte* backend, void* motion_rtv) noexcept
{
    bound_ = false;
    if (!backend || !motion_rtv) return BindAction::None;
    void* slot{};
    bool changed{};
    if (!host_.read(backend + layout_.rt_offset + 2 * sizeof(void*), &slot, sizeof(slot)) ||
        !host_.read(backend + layout_.changed_offset, &changed, sizeof(changed))) return BindAction::Fault;
    if (slot != motion_rtv) return BindAction::None;
    void* const none{};
    const bool dirty = true;
    if (!host_.write(backend + layout_.rt_offset + 2 * sizeof(void*), &none, sizeof(none)) ||
        !host_.write(backend + layout_.changed_offset, &dirty, sizeof(dirty))) return BindAction::Fault;
    // Same as CBackend::set_RT: the first change of a clean cache unbinds everything, and ApplyRTandZB rebinds the
    // whole (now motion-free) cache before the next draw.
    if (!changed) host_.clear_targets();
    return BindAction::Unbound;
}
}
