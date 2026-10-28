// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <cstddef>
#include <cstdint>
#include <rescaleframe/game_frame.h>

namespace rsf_gta5_enhanced {
// Native Streamline 2.x structures observed at the game's own calls. These
// describe producer inputs, not ownership transferred to the runtime.
struct NativeHeader {
    void* next;
    unsigned char type[16];
    uint32_t version, padding;
};
struct NativeConstants {
    NativeHeader header;
    float projection[16], inverse_projection[16], lens[16];
    float clip_to_previous[16], previous_to_clip[16];
    float jitter[2], motion_scale[2], pinhole[2];
    float position[3], up[3], right[3], forward[3];
    float near_plane, far_plane, fov, aspect, invalid_motion;
    uint8_t depth_inverted, camera_motion, motion_3d, reset, orthographic, dilated, jittered;
    float min_relative_depth;
};
struct NativeResource {
    NativeHeader header;
    uint8_t type, padding[7];
    void* native;
    void* memory;
    void* view;
    uint32_t state, width, height, format, mips, layers;
};
struct NativeTag {
    NativeHeader header;
    NativeResource* resource;
    uint32_t type, lifecycle;
    uint32_t top, left, width, height;
};
static_assert(sizeof(NativeHeader) == 32);
static_assert(offsetof(NativeConstants, jitter) == 352 && offsetof(NativeConstants, position) == 376);
static_assert(offsetof(NativeConstants, near_plane) == 424 && offsetof(NativeConstants, depth_inverted) == 444);
static_assert(offsetof(NativeResource, native) == 40 && offsetof(NativeResource, state) == 64);
static_assert(sizeof(NativeTag) == 64);

bool decode_camera(const NativeConstants&, uint32_t render_width, uint32_t render_height,
    uint32_t output_width, uint32_t output_height, float frame_seconds,
    const float previous_jitter[2], rsf_camera_frame&) noexcept;
}
