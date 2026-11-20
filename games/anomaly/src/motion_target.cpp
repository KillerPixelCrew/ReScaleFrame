// SPDX-License-Identifier: MIT
#include "motion_target.h"

namespace rsf::anomaly::motion {
namespace {
constexpr rsf_anomaly_motion_bind_platform platform{
    [](void* host, const void* address, void* out, size_t size) {
        return uint32_t(static_cast<BinderHost*>(host)->read(address, out, size));
    },
    [](void* host, void* address, const void* in, size_t size) {
        return uint32_t(static_cast<BinderHost*>(host)->write(address, in, size));
    },
    [](void* host, void* const* targets, void* depth) {
        static_cast<BinderHost*>(host)->set_targets(*reinterpret_cast<void* const (*)[4]>(targets), depth);
    },
    [](void* host) { static_cast<BinderHost*>(host)->clear_targets(); },
};
}

BindAction MotionBinder::on_draw(std::byte* backend, bool motion_wanted, const void* position_rtv, void* motion_rtv) noexcept
{
    return static_cast<BindAction>(rsf_anomaly_motion_bind_draw(
        &state_, &platform, &host_, &layout_, backend, motion_wanted, position_rtv, motion_rtv));
}

BindAction MotionBinder::close(std::byte* backend, void* motion_rtv) noexcept
{
    return static_cast<BindAction>(rsf_anomaly_motion_bind_close(&state_, &platform, &host_, &layout_, backend, motion_rtv));
}
}
