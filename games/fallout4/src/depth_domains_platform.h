// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between the Fallout 4 depth-domain leaves (depth_domains.cpp) and their Rust
// owner (depth_domains.rs). Not a DLL ABI: both sides link into ReScaleFrame.Game.Fallout4.dll.
// depth_domains.rs mirrors every structure here and asserts the same layout.
#include "depth_domains.h"
#include <cstddef>

namespace rsf::fallout4 {
// One Texture2D as D3D11 reports it. Identities are canonical IUnknown pointers, 0 when absent;
// two live objects are the same object exactly when their identities are equal and nonzero.
struct NativeDepthTextureFacts {
    uint64_t resource{}, device{};
    uint32_t texture{}, width{}, height{}, format{}, mip_levels{}, array_size{};
    uint32_t sample_count{}, sample_quality{};
};
// viewport_copied is the second RSGetViewports count, made only when the first returned one.
struct NativeDepthContextFacts {
    uint64_t device{};
    uint32_t immediate{}, viewport_count{}, viewport_copied{};
    float viewport[6]{};
};
// The bound OM slot0 and depth views. rtv and dsv are the raw bound pointers.
struct NativeDepthTargetFacts {
    uint64_t rtv{}, dsv{}, rtv_identity{};
    NativeDepthTextureFacts depth{}, colour{};
    uint32_t dsv_format{}, dsv_dimension{}, dsv_mip{}, rtv_dimension{}, rtv_mip{};
};
// Scissor rectangles are queried only when the bound rasterizer enables them.
struct NativeDepthScissorFacts {
    uint32_t enabled{}, count{}, copied{};
    int32_t rect[4]{};
};
struct NativeDepthViewFacts {
    uint64_t resource{}, device{};
    uint32_t format{}, dimension{}, most_detailed_mip{}, mip_levels{};
};
struct NativeDepthMathFailure {
    uint32_t index{};
    float actual{}, expected{}, relative_error{};
    double w[4]{}, minimum_w{}, range{};
};
// Leaves the Rust owner cannot express without C++ or COM. None of them throws. Rust owns the
// engine reads, every admission decision, the camera math and the composed domains.
struct NativeDepthLeaves {
    // gs:[0x58], this thread's TLS slot array.
    uintptr_t (*tls_slots)();
    // Canonical IUnknown identity of a live COM object.
    uint64_t (*identity)(uint64_t object);
    void (*context)(uint64_t context, NativeDepthContextFacts*);
    void (*targets)(uint64_t context, NativeDepthTargetFacts*);
    void (*scissor)(uint64_t context, NativeDepthScissorFacts*);
    // A typed ID3D11Texture2D, described without a Texture2D query.
    void (*texture)(uint64_t texture, NativeDepthTextureFacts*);
    void (*view)(uint64_t view, NativeDepthViewFacts*);
    // Guarded receipts. math is 0 for none, 1 for the projection text and 2 for the W text.
    void (*refusal)(NativeDepthReport, const char* stage, const NativeDepthSnapshot*,
        const NativeDepthMathFailure*, uint32_t math);
    void (*lease)(NativeDepthReport, const NativeDepthSnapshot* snapshot,
        const NativeDepthSnapshot* current, uint32_t reached, uint32_t passed, uint32_t stage,
        uint32_t changed);
};

static_assert(sizeof(NativeDepthTextureFacts) == 48 && offsetof(NativeDepthTextureFacts, texture) == 16 &&
    offsetof(NativeDepthTextureFacts, sample_quality) == 44);
static_assert(sizeof(NativeDepthContextFacts) == 48 && offsetof(NativeDepthContextFacts, viewport) == 20);
static_assert(sizeof(NativeDepthTargetFacts) == 144 && offsetof(NativeDepthTargetFacts, depth) == 24 &&
    offsetof(NativeDepthTargetFacts, colour) == 72 && offsetof(NativeDepthTargetFacts, dsv_format) == 120 &&
    offsetof(NativeDepthTargetFacts, rtv_mip) == 136);
static_assert(sizeof(NativeDepthScissorFacts) == 28 && offsetof(NativeDepthScissorFacts, rect) == 12);
static_assert(sizeof(NativeDepthViewFacts) == 32 && offsetof(NativeDepthViewFacts, mip_levels) == 28);
static_assert(sizeof(NativeDepthMathFailure) == 64 && offsetof(NativeDepthMathFailure, w) == 16 &&
    offsetof(NativeDepthMathFailure, range) == 56);
static_assert(sizeof(NativeDepthLeaves) == 9 * sizeof(void*));
}
