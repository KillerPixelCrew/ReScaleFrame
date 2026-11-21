// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <rescaleframe/game_frame.h>
namespace rsf::skyrim {
struct NativeCamera {
    float view[16], projection[16], view_projection[16], previous_view_projection[16];
    float origin[3], previous_origin[3], jitter_ndc[2];
    float near_plane, far_plane;
};
static_assert(sizeof(NativeCamera) == 296);
static_assert(offsetof(NativeCamera, origin) == 256);
static_assert(offsetof(NativeCamera, jitter_ndc) == 280);

extern "C" bool rsf_skyrim_camera_frame(const NativeCamera*, uint32_t width, uint32_t height,
    rsf_camera_frame*, float* previous_clip_to_clip, bool* history_valid) noexcept;

// Native cache matrices act on row vectors in camera-relative world space.
inline bool camera_frame(const NativeCamera& native, uint32_t width, uint32_t height,
    rsf_camera_frame& out, float previous_clip_to_clip[16], bool& history_valid) noexcept
{
    return rsf_skyrim_camera_frame(&native, width, height, &out, previous_clip_to_clip, &history_valid);
}
}
