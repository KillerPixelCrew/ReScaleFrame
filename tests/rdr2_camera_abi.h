// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_frame.h>
#include <cstddef>

// The decode is owned by the Rust crate rsf-rdr2 (games/rdr2/src/camera.rs). These mirror its
// repr(C) CameraInputs and CameraResult for the native camera fixture.
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
static_assert(sizeof(rsf_rdr2_camera_inputs) == 40);
static_assert(sizeof(rsf_rdr2_camera_result) == sizeof(rsf_camera_frame) + 68);

extern "C" bool rsf_rdr2_decode_render_camera_abi(const void* view, size_t bytes,
    const rsf_rdr2_camera_inputs* input, rsf_rdr2_camera_result* result) noexcept;

// The caller copies the active worker view at the upscale boundary. No game pointer is retained.
// Matrices retain RAGE's negative-Z-forward view convention and Z-up world coordinates.
inline bool rsf_rdr2_decode_render_camera(const void* view, size_t bytes,
    const rsf_rdr2_camera_inputs& input, rsf_rdr2_camera_result& result) noexcept
{
    return rsf_rdr2_decode_render_camera_abi(view, bytes, &input, &result);
}
