// SPDX-License-Identifier: MIT
#pragma once
#include <cstdint>

// The receipt leaves print this record. Rust owns the field comparisons.
// render_inputs_platform.h asserts the matching layout.
namespace rsf::fallout4::detail {
struct NativeState {
    uintptr_t renderer{}, camera{}, cache{}, context{}, device{}, chain{}, window{};
    uint32_t frame{}, width{}, height{}, count{}, taa{}, disable{}, colour{}, motion{}, depth{};
    float ratio[2]{}, viewport[4]{}, delta_seconds{};
    unsigned char default_viewport{}, dynamic_viewport{};
};
}
