/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>

/* Private SR payloads. The C++ boundary constructs the versioned SDK objects
   and verifies the ranges copied between these records and their members. */
typedef struct rsf_sl_sr_dimensions {
    uint32_t mode, width, height;
} rsf_sl_sr_dimensions;

typedef struct rsf_sl_sr_options {
    rsf_sl_sr_dimensions dimensions;
    uint32_t presets[6];
    uint8_t hdr, auto_exposure, alpha;
} rsf_sl_sr_options;

typedef struct rsf_sl_sr_optimal {
    uint32_t width, height;
    float sharpness;
    uint32_t width_min, height_min, width_max, height_max;
} rsf_sl_sr_optimal;

typedef struct rsf_sl_sr_constants {
    float view_to_clip[16], clip_to_view[16];
    float clip_to_previous[16], previous_to_clip[16];
    float jitter[2], motion_scale[2], pinhole[2];
    float position[3], up[3], right[3], forward[3];
    float near_plane, far_plane, fov, aspect, invalid_motion;
    uint8_t depth_inverted, camera_motion, motion_3d, reset;
} rsf_sl_sr_constants;
