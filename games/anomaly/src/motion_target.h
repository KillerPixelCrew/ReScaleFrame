// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// RTV2 bind preservation for the G-buffer pass. Pure state machine over the engine's RCache render-target cache
// (CBackend::pRT[4], pZB, m_bChangedRTorZB; sr_profile.h backend_*_offset), no D3D types.
//
// The engine binds through CBackend::set_RT (cache + dirty flag) and ApplyRTandZB, which issues one
// OMSetRenderTargets(4, pRT, pZB) before constants.flush. The G-buffer bind is u_setrt(rt_Position, rt_Color, 0, zb),
// so pRT[2] is null there. At each draw-time flush inside the window, when pRT[0] is rt_Position's RTV and pRT[2] is
// null, the motion RTV is written into pRT[2] and the same four targets are bound, so the engine's cache stays the
// truth: its next set_RT(x, 2) compares against the motion RTV and rebinds, and every later G-buffer bind is
// augmented again (a "rebind"). A draw outside the window or with another RT0 while pRT[2] still holds the motion
// RTV gets it removed the same way (an "unbind"), so the motion target never reaches a pass of another size.
// A non-null foreign pRT[2] is never replaced ("conflict").
#include <cstddef>
#include <cstdint>

namespace rsf::anomaly::motion {
class BinderHost {
public:
    virtual ~BinderHost() = default;
    virtual bool read(const void* address, void* out, std::size_t size) noexcept = 0;
    virtual bool write(void* address, const void* in, std::size_t size) noexcept = 0;
    // OMSetRenderTargets(4, targets, depth).
    virtual void set_targets(void* const (&targets)[4], void* depth) noexcept = 0;
    // OMSetRenderTargets(0, nullptr, nullptr), as CBackend::set_RT does when it first dirties the cache.
    virtual void clear_targets() noexcept = 0;
};
enum class BindAction : std::uint8_t {
    None,          // not a G-buffer draw and nothing of ours bound
    Bound,         // pRT[2] was null: motion RTV written and bound (a rebind after an engine bind)
    AlreadyBound,  // pRT[2] already the motion RTV
    Unbound,       // the motion RTV was removed from a non-G-buffer draw
    Conflict,      // a G-buffer draw with a foreign RTV in slot 2: left alone, no motion for this draw
    Fault          // engine memory unreadable/unwritable: nothing changed
};
struct BackendLayout {
    std::uint32_t rt_offset{}, zb_offset{}, changed_offset{};
};
class MotionBinder {
public:
    MotionBinder(BinderHost& host, BackendLayout layout) noexcept : host_(host), layout_(layout) {}
    // At the draw-time flush (after ApplyRTandZB). `motion_wanted`: window open and target usable for this frame.
    BindAction on_draw(std::byte* backend, bool motion_wanted, const void* position_rtv, void* motion_rtv) noexcept;
    // At window close, outside a draw: drop our RTV from the cache and mark it dirty like set_RT does.
    BindAction close(std::byte* backend, void* motion_rtv) noexcept;
    bool motion_bound() const noexcept { return bound_; }
private:
    BinderHost& host_;
    BackendLayout layout_;
    bool bound_{};
};
}
