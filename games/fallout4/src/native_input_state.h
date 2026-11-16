// SPDX-License-Identifier: MIT
#pragma once
#include <bit>
#include <cstdint>

namespace rsf::fallout4::detail {
struct NativeState {
    uintptr_t renderer{}, camera{}, cache{}, context{}, device{}, chain{}, window{};
    uint32_t frame{}, width{}, height{}, count{}, taa{}, disable{}, colour{}, motion{}, depth{};
    float ratio[2]{}, viewport[4]{}, delta_seconds{};
    unsigned char default_viewport{}, dynamic_viewport{};
};
// owner() reads only these fields. Camera/temporal state is not an owner lease.
inline bool same_native_owner(const NativeState& a, const NativeState& b) noexcept {
    return a.renderer==b.renderer && a.device==b.device && a.context==b.context &&
        a.chain==b.chain && a.window==b.window && a.frame==b.frame && a.width==b.width && a.height==b.height;
}
inline bool same_native_state(const NativeState& a, const NativeState& b) noexcept {
    if (!same_native_owner(a,b) || a.camera!=b.camera || a.cache!=b.cache || a.count!=b.count ||
        a.taa!=b.taa || a.disable!=b.disable || a.colour!=b.colour || a.motion!=b.motion || a.depth!=b.depth ||
        a.default_viewport!=b.default_viewport || a.dynamic_viewport!=b.dynamic_viewport) return false;
    // Preserve the old exact float-field comparison, including signed zero,
    // without comparing any object padding or unread fields.
    for (unsigned i=0;i<2;++i)
        if (std::bit_cast<uint32_t>(a.ratio[i])!=std::bit_cast<uint32_t>(b.ratio[i])) return false;
    for (unsigned i=0;i<4;++i)
        if (std::bit_cast<uint32_t>(a.viewport[i])!=std::bit_cast<uint32_t>(b.viewport[i])) return false;
    // Frame timing is sampled in the first snapshot. The native timing global
    // can advance during the same render frame without changing its inputs.
    return true;
}
}
