// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between the Fallout 4 render-input leaves (render_inputs.cpp) and their Rust
// owner (render_inputs.rs). Not a DLL ABI: both sides link into ReScaleFrame.Game.Fallout4.dll.
// render_inputs.rs mirrors every structure here and asserts the same layout.
#include "depth_domains.h"
#include "native_input_state.h"
#include <rescaleframe/game_renderer.h>
#include <cstddef>

namespace rsf::fallout4 {
struct NativeCamera {
    float view[16]{}, projection[16]{};
    float view_projection_unjittered[16]{}, previous_view_projection_unjittered[16]{};
    float position_adjust[3]{}, previous_position_adjust[3]{}, jitter_ndc[2]{};
};
struct CameraRaster {
    float xywh[4]{};
    // Integer pixel edges, LTRB; extents are right-left and bottom-top.
    int32_t crop[4]{};
};
static_assert(sizeof(NativeCamera) == 288);
static_assert(offsetof(NativeCamera, projection) == 64);
static_assert(offsetof(NativeCamera, view_projection_unjittered) == 128);
static_assert(offsetof(NativeCamera, previous_view_projection_unjittered) == 192);
static_assert(offsetof(NativeCamera, position_adjust) == 256);
static_assert(offsetof(NativeCamera, previous_position_adjust) == 268);
static_assert(offsetof(NativeCamera, jitter_ndc) == 280);
static_assert(sizeof(CameraRaster) == 32 && offsetof(CameraRaster, crop) == 16);

using NativeInputReport = void (*)(const char*);

// Texture slots of the reader lease. Slot 0 also holds the framebuffer texture.
enum NativeInputSlot : uint32_t {
    native_input_colour, native_input_motion, native_input_depth,
    native_input_refraction_normals, native_input_pipboy, native_input_vats,
    native_input_slot_count, native_input_no_slot = 0xffffffffu
};

struct NativeInputSwapFacts {
    uint64_t window{};
    uint32_t read{}, width{}, height{}, reserved{};
};
// The resource behind one shader-resource or render-target view. identity is the viewed
// resource's; pointer is the ID3D11Texture2D query result, 0 when the resource is missing or the
// query failed. device and the allocation fields describe that texture. view_mip is MostDetailedMip
// for a shader-resource view and MipSlice for a render-target view; view_mips is 0 for the latter.
struct NativeInputResourceFacts {
    uint64_t identity{}, pointer{}, device{};
    uint32_t read{}, width{}, height{}, format{}, mip_levels{}, array_size{};
    uint32_t sample_count{}, sample_quality{}, bind_flags{};
    uint32_t view_format{}, view_dimension{}, view_mip{}, view_mips{}, reserved{};
};
// Every local the reader receipt prints, as Rust last wrote it.
struct NativeInputReceipt {
    detail::NativeState before{};
    NativeCamera native{};
    NativeInputResourceFacts views[3]{}, rtv{};
    NativeInputSwapFacts swap{};
    uint64_t expected_renderer{}, selected{}, last_key{}, expected[3]{};
    uint32_t after_frame{}, scanned{}, camera_matches{}, matching_jitter{}, last_jitter{};
    uint32_t viewport_count{}, confirmed_viewport_count{}, camera_read{};
    uint32_t resource_match[3]{}, device_match[3]{};
    float viewport[6]{}, confirmed_viewport[6]{};
};
struct NativeInputHistoryReceipt {
    uint32_t frame{}, previous_frame{}, reset{}, reasons{};
    uint32_t width{}, height{}, previous_width{}, previous_height{};
    CameraRaster raster{}, previous_raster{};
};
// The framebuffer receipt. Allocation, view and swap fields are filled at the stage that queried
// them in the former reader.
struct NativeFramebufferReceipt {
    detail::NativeState before{}, after{};
    NativeInputSwapFacts swap{};
    uint64_t target_texture{}, target_rtv{}, leased{};
    uint32_t platform{}, confirmed_platform{};
    uint32_t width{}, height{}, mip_levels{}, array_size{}, sample_count{}, format{};
    uint32_t rtv_dimension{}, rtv_mip{};
};
struct NativeFramebufferFacts {
    uint64_t native_frame{}, window_key{};
};

// Leaves the Rust owner cannot express without C++ or COM. None of them throws. Rust owns the
// engine reads, the code guards, every admission decision, the camera history and the passes.
// lease is the caller's NativeInputLease (render_inputs.cpp); it holds typed COM references until
// the wrapper moves them into its output or discards them.
struct NativeInputLeaves {
    // Guarded receipts with the former text. Rust keeps the stage masks and budgets.
    void (*reader)(NativeInputReport, const char* stage, const NativeInputReceipt*,
        const rsf_game_render_pass*);
    void (*mismatch)(NativeInputReport, const detail::NativeState* before,
        const detail::NativeState* after, uint32_t mask);
    void (*history)(NativeInputReport, const NativeInputHistoryReceipt*);
    void (*framebuffer)(NativeInputReport, const char* stage, const char* owner_stage,
        const NativeFramebufferReceipt*);
};

struct NativeInputLease;

extern "C" {
bool rsf_fallout4_inputs_claim_exception() noexcept;
bool rsf_fallout4_inputs_read(const NativeInputLeaves*, const NativeDepthLeaves*, NativeInputLease* lease,
    NativeInputReport, const NativeDepthSnapshot* producer, rsf_game_render_pass*) noexcept;
bool rsf_fallout4_inputs_read_raster(NativeInputLease* lease,
    const rsf_game_render_pass* frame, const NativeDepthSnapshot* producer, rsf_game_render_pass*) noexcept;
bool rsf_fallout4_inputs_read_framebuffer(const NativeInputLeaves*, NativeInputLease* lease, NativeInputReport,
    NativeFramebufferFacts*) noexcept;
}

static_assert(sizeof(detail::NativeState) == 128 && offsetof(detail::NativeState, frame) == 56 &&
    offsetof(detail::NativeState, ratio) == 92 && offsetof(detail::NativeState, viewport) == 100 &&
    offsetof(detail::NativeState, delta_seconds) == 116 && offsetof(detail::NativeState, default_viewport) == 120 &&
    offsetof(detail::NativeState, dynamic_viewport) == 121);
static_assert(sizeof(NativeInputSwapFacts) == 24 && offsetof(NativeInputSwapFacts, read) == 8);
static_assert(sizeof(NativeInputResourceFacts) == 80 && offsetof(NativeInputResourceFacts, read) == 24 &&
    offsetof(NativeInputResourceFacts, view_format) == 60 && offsetof(NativeInputResourceFacts, reserved) == 76);
static_assert(sizeof(NativeInputReceipt) == 912 && offsetof(NativeInputReceipt, native) == 128 &&
    offsetof(NativeInputReceipt, views) == 416 && offsetof(NativeInputReceipt, rtv) == 656 &&
    offsetof(NativeInputReceipt, swap) == 736 && offsetof(NativeInputReceipt, expected_renderer) == 760 &&
    offsetof(NativeInputReceipt, expected) == 784 && offsetof(NativeInputReceipt, after_frame) == 808 &&
    offsetof(NativeInputReceipt, camera_read) == 836 && offsetof(NativeInputReceipt, device_match) == 852 &&
    offsetof(NativeInputReceipt, viewport) == 864 && offsetof(NativeInputReceipt, confirmed_viewport) == 888);
static_assert(sizeof(NativeInputHistoryReceipt) == 96 && offsetof(NativeInputHistoryReceipt, raster) == 32);
static_assert(sizeof(NativeFramebufferReceipt) == 344 && offsetof(NativeFramebufferReceipt, swap) == 256 &&
    offsetof(NativeFramebufferReceipt, target_texture) == 280 && offsetof(NativeFramebufferReceipt, platform) == 304 &&
    offsetof(NativeFramebufferReceipt, rtv_mip) == 340);
static_assert(sizeof(NativeFramebufferFacts) == 16);
static_assert(sizeof(NativeInputLeaves) == 4 * sizeof(void*));
}
