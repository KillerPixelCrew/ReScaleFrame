// SPDX-License-Identifier: GPL-3.0-only
#include "../games/gta5-enhanced/src/native_inputs.h"
#include <cmath>
#include <cstring>
#include <cstdio>

int main()
{
    using namespace rsf_gta5_enhanced;
    NativeConstants native{}; native.header.version = 2;
    const float projection[] = {1.34031677f,0,0,0, 0,2.14450693f,0,0,
        0,0,1.00000998e-5f,-1, 0,0,0.100001f,0};
    const float inverse[] = {0.74609226f,0,0,0, 0,0.46630767f,0,0,
        0,0,0,9.99989986f, 0,0,-0.99999994f,9.99999975e-5f};
    std::memcpy(native.projection, projection, sizeof(projection));
    std::memcpy(native.inverse_projection, inverse, sizeof(inverse));
    for (unsigned i = 0; i < 16; i += 5) native.clip_to_previous[i] = native.previous_to_clip[i] = 1;
    native.position[0] = 6.62053347f; native.position[1] = 534.458862f; native.position[2] = 176.56752f;
    const float right[] = {0.91754359f,-0.397635251f,0};
    const float up[] = {0.00898732711f,0.0207382627f,0.999744534f};
    const float forward[] = {-0.397533655f,-0.917309165f,0.0226019379f};
    std::memcpy(native.right, right, sizeof(right)); std::memcpy(native.up, up, sizeof(up));
    std::memcpy(native.forward, forward, sizeof(forward));
    native.near_plane = 0.1f; native.far_plane = 10000; native.fov = 0.87266463f;
    native.depth_inverted = 1; native.camera_motion = 1;
    native.jitter[0] = -0.0625f; native.jitter[1] = 0.462962955f;
    const float previous_jitter[] = {0.5f,0.5f};
    rsf_camera_frame camera{};
    if (!decode_camera(native,1707,1067,2560,1600,0.01f,previous_jitter,camera)) return 1;
    for (unsigned i = 0; i < 3; ++i) {
        if (camera.view_to_world[12+i] != native.position[i] ||
            camera.view_to_world[i] != native.right[i] || camera.view_to_world[4+i] != native.up[i] ||
            -camera.view_to_world[8+i] != native.forward[i]) return 1;
    }
    if (camera.jitter_pixels[0] != native.jitter[0] || camera.previous_jitter_pixels[1] != 0.5f ||
        camera.output_width != 2560 || !camera.depth_inverted) return 1;
    native.inverse_projection[0] = 0;
    if (decode_camera(native,1707,1067,2560,1600,0.01f,previous_jitter,camera)) return 1;
    native.inverse_projection[0] = inverse[0]; native.up[0] = 5;
    if (decode_camera(native,1707,1067,2560,1600,0.01f,previous_jitter,camera)) return 1;
    std::puts("GTA camera: measured projection/pose round-trip, jitter and invalid-input rejection passed.");
    return 0;
}
