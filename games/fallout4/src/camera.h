// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <rescaleframe/game_frame.h>

// The math lives in the rsf-fallout4 crate (camera.rs). These wrappers keep the C++ signatures.
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
struct CameraDepthBound {
    double coefficients[4]{}, minimum{}, range{};
};
static_assert(sizeof(NativeCamera) == 288);
static_assert(offsetof(NativeCamera, projection) == 64);
static_assert(offsetof(NativeCamera, view_projection_unjittered) == 128);
static_assert(offsetof(NativeCamera, previous_view_projection_unjittered) == 192);
static_assert(offsetof(NativeCamera, position_adjust) == 256);
static_assert(offsetof(NativeCamera, previous_position_adjust) == 268);
static_assert(offsetof(NativeCamera, jitter_ndc) == 280);
static_assert(sizeof(CameraRaster) == 32 && offsetof(CameraRaster, crop) == 16);
static_assert(sizeof(CameraDepthBound) == 48 && offsetof(CameraDepthBound, minimum) == 32 &&
    offsetof(CameraDepthBound, range) == 40);

extern "C" {
bool rsf_fallout4_camera_raster_transform(const CameraRaster*, float* output16) noexcept;
bool rsf_fallout4_camera_motion_to_uv(const CameraRaster*, float* output2) noexcept;
bool rsf_fallout4_camera_depth_domain_forward(const float* matrix16, float range,
    CameraDepthBound*) noexcept;
bool rsf_fallout4_camera_composed_view_depth(const float* source_projection16,
    const float* target_projection16, float* source_clip_to_target_clip16) noexcept;
bool rsf_fallout4_camera_frame(const NativeCamera*, uint32_t width, uint32_t height,
    rsf_camera_frame*, float* previous_clip_to_clip, bool* history_valid,
    const CameraRaster* current, const CameraRaster* previous) noexcept;
}

// Native NDC -> integer crop NDC affine, row-vector convention.
inline bool camera_raster_transform(const CameraRaster& raster, float* output16) noexcept {
    return rsf_fallout4_camera_raster_transform(&raster, output16);
}
// Native previous-minus-current UV -> current-minus-previous crop UV,
// valid for steady raster history. Raster changes independently reset history.
inline bool camera_motion_to_uv(const CameraRaster& raster, float* output2) noexcept {
    return rsf_fallout4_camera_motion_to_uv(&raster, output2);
}
// D24 source clip-box forward-W admission, with one endpoint quantization step.
inline bool camera_depth_domain_forward(const float* matrix16, float range,
    CameraDepthBound& output) noexcept {
    return rsf_fallout4_camera_depth_domain_forward(matrix16, range, &output);
}
// Composed first-person view-Z convention, not a shared-world point transform.
// Source is the actual jittered near projection; target is unjittered world P.
inline bool camera_composed_view_depth(const float* source_projection16,
    const float* target_projection16, float* source_clip_to_target_clip16) noexcept {
    return rsf_fallout4_camera_composed_view_depth(source_projection16, target_projection16,
        source_clip_to_target_clip16);
}
inline bool camera_frame(const NativeCamera& native, uint32_t width, uint32_t height,
    rsf_camera_frame& output, float* previous_clip_to_clip, bool& history_valid,
    const CameraRaster* current = nullptr, const CameraRaster* previous = nullptr) noexcept {
    return rsf_fallout4_camera_frame(&native, width, height, &output, previous_clip_to_clip,
        &history_valid, current, previous);
}
}
