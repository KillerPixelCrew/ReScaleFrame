// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between the Fallout 4 depth-domain leaves (depth_domains.cpp) and their Rust
// owner (depth_domains.rs). Not a DLL ABI: both sides link into ReScaleFrame.Game.Fallout4.dll.
// depth_domains.rs mirrors every structure here and asserts the same layout.
#include "depth_domains.h"
#include <cstddef>

namespace rsf::fallout4 {
struct NativeDepthMathFailure {
    uint32_t index{};
    float actual{}, expected{}, relative_error{};
    double w[4]{}, minimum_w{}, range{};
};
struct NativeDepthLeaves {
    // Guarded receipts. math is 0 for none, 1 for the projection text and 2 for the W text.
    void (*refusal)(NativeDepthReport, const char* stage, const NativeDepthSnapshot*,
        const NativeDepthMathFailure*, uint32_t math);
    void (*lease)(NativeDepthReport, const NativeDepthSnapshot* snapshot,
        const NativeDepthSnapshot* current, uint32_t reached, uint32_t passed, uint32_t stage,
        uint32_t changed);
};

static_assert(sizeof(NativeDepthMathFailure) == 64 && offsetof(NativeDepthMathFailure, w) == 16 &&
    offsetof(NativeDepthMathFailure, range) == 56);
static_assert(sizeof(NativeDepthLeaves) == 2 * sizeof(void*));
}
