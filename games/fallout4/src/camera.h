// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_frame.h>

namespace rsf::fallout4 {
struct NativeCamera {
    float view[16]{}, projection[16]{};
    float view_projection_unjittered[16]{}, previous_view_projection_unjittered[16]{};
    float position_adjust[3]{}, previous_position_adjust[3]{}, jitter_ndc[2]{};
};
bool camera_frame(const NativeCamera&, uint32_t width, uint32_t height,
    rsf_camera_frame&, float* previous_clip_to_clip, bool& history_valid) noexcept;
}
