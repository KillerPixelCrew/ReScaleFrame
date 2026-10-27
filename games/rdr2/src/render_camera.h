// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_frame.h>
#include <cstddef>

struct rsf_rdr2_camera_inputs {
    uint32_t render_width{}, render_height{}, output_width{}, output_height{};
    float jitter[2]{}, previous_jitter[2]{};
    float frame_time_seconds{};
    bool reset{}, motion_jittered{};
};
struct rsf_rdr2_camera_result {
    rsf_camera_frame camera{};
    float previous_clip_to_clip[16]{};
    bool projection_jitter_matches{};
};
// The caller copies the active worker view at the upscale boundary. No game pointer is retained.
// Matrices retain RAGE's negative-Z-forward view convention and Z-up world coordinates.
bool rsf_rdr2_decode_render_camera(const void* view, size_t bytes,
    const rsf_rdr2_camera_inputs&, rsf_rdr2_camera_result&) noexcept;
