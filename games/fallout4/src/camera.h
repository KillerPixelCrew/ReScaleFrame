// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_frame.h>

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
// Native NDC -> integer crop NDC affine, row-vector convention.
bool camera_raster_transform(const CameraRaster&, float* output16) noexcept;
// Native previous-minus-current UV -> current-minus-previous crop UV,
// valid for steady raster history. Raster changes independently reset history.
bool camera_motion_to_uv(const CameraRaster&, float* output2) noexcept;
struct CameraDepthBound {
    double coefficients[4]{}, minimum{}, range{};
};
// D24 source clip-box forward-W admission, with one endpoint quantization step.
bool camera_depth_domain_forward(const float* matrix16, float range, CameraDepthBound&) noexcept;
// Composed first-person view-Z convention, not a shared-world point transform.
// Source is the actual jittered near projection; target is unjittered world P.
bool camera_composed_view_depth(const float* source_projection16,
    const float* target_projection16, float* source_clip_to_target_clip16) noexcept;
bool camera_frame(const NativeCamera&, uint32_t width, uint32_t height,
    rsf_camera_frame&, float* previous_clip_to_clip, bool& history_valid,
    const CameraRaster* current = nullptr, const CameraRaster* previous = nullptr) noexcept;
}
