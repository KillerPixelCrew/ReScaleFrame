// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_frame.h>
namespace rsf::skyrim {
struct NativeCamera {
    float view[16], projection[16], view_projection[16], previous_view_projection[16];
    float origin[3], previous_origin[3], jitter_ndc[2];
    float near_plane, far_plane;
};
// Native cache matrices act on row vectors in camera-relative world space.
bool camera_frame(const NativeCamera&, uint32_t width, uint32_t height,
    rsf_camera_frame&, float previous_clip_to_clip[16], bool& history_valid);
}
