// SPDX-License-Identifier: GPL-3.0-only
#include "native_inputs.h"
#include <cmath>
#include <cstring>

namespace rsf_gta5_enhanced {
namespace {
bool inverse_pair(const float* a, const float* b) noexcept
{
    for (unsigned row = 0; row < 4; ++row) for (unsigned column = 0; column < 4; ++column) {
        float value = 0;
        for (unsigned k = 0; k < 4; ++k) value += a[row * 4 + k] * b[k * 4 + column];
        if (!std::isfinite(value) || std::abs(value - (row == column ? 1.f : 0.f)) > 0.002f) return false;
    }
    return true;
}
}
bool decode_camera(const NativeConstants& native, uint32_t width, uint32_t height,
    uint32_t output_width, uint32_t output_height, float seconds,
    const float previous_jitter[2], rsf_camera_frame& output) noexcept
{
    if (!width || !height || !output_width || !output_height || native.header.version < 1 ||
        native.header.version > 2 || !std::isfinite(seconds) || seconds <= 0 || seconds > 1 ||
        !std::isfinite(native.near_plane) || native.near_plane <= 0 ||
        !std::isfinite(native.far_plane) || native.far_plane <= native.near_plane ||
        !std::isfinite(native.fov) || native.fov <= 0 || native.fov >= 3.141593f ||
        !inverse_pair(native.projection, native.inverse_projection) ||
        !inverse_pair(native.clip_to_previous, native.previous_to_clip)) return false;
    rsf_camera_frame camera{};
    camera.struct_size = sizeof(camera); camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    std::memcpy(camera.view_to_clip, native.projection, sizeof(camera.view_to_clip));
    std::memcpy(camera.clip_to_view, native.inverse_projection, sizeof(camera.clip_to_view));
    std::memcpy(camera.clip_to_previous_clip, native.clip_to_previous, sizeof(camera.clip_to_previous_clip));
    // Canonical RSF view Z is opposite look direction for this w=-viewZ
    // projection. Preserve the native SL pose exactly when a backend rebuilds
    // its common constants; clip history is supplied directly, never guessed.
    for (unsigned i = 0; i < 3; ++i) {
        const float values[] = {native.right[i], native.up[i], -native.forward[i], native.position[i]};
        for (float value : values) if (!std::isfinite(value)) return false;
        camera.view_to_world[i] = values[0];
        camera.view_to_world[4 + i] = values[1];
        camera.view_to_world[8 + i] = values[2];
        camera.view_to_world[12 + i] = values[3];
    }
    camera.view_to_world[15] = camera.world_to_view[15] = 1;
    for (unsigned row = 0; row < 3; ++row) {
        float norm = 0;
        for (unsigned column = 0; column < 3; ++column) {
            const float value = camera.view_to_world[row * 4 + column];
            norm += value * value;
            camera.world_to_view[column * 4 + row] = value;
            camera.world_to_view[12 + row] -= native.position[column] * value;
        }
        if (std::abs(norm - 1) > 0.002f) return false;
    }
    if (!inverse_pair(camera.view_to_world, camera.world_to_view)) return false;
    for (unsigned i = 0; i < 2; ++i) {
        if (!std::isfinite(native.jitter[i]) || !std::isfinite(previous_jitter[i])) return false;
        camera.jitter_pixels[i] = native.jitter[i];
        camera.previous_jitter_pixels[i] = previous_jitter[i];
    }
    camera.render_width = width; camera.render_height = height;
    camera.output_width = output_width; camera.output_height = output_height;
    camera.near_plane = native.near_plane; camera.far_plane = native.far_plane;
    camera.vertical_fov_radians = native.fov; camera.depth_inverted = native.depth_inverted == 1;
    camera.motion_jittered = native.jittered == 1; camera.frame_time_seconds = seconds;
    output = camera;
    return true;
}
}
