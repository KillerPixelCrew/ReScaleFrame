// SPDX-License-Identifier: GPL-3.0-only
#include "../games/skyrim-se/src/camera.h"
#include <cmath>
#include <cstdio>
#include <cstring>
#include <limits>
#include <algorithm>
#include <initializer_list>
namespace {
int failures{};
void check(bool ok, const char* name) { if (!ok) { ++failures; std::printf("FAIL %s\n", name); } }
float transformed(const float* point, const float* matrix, unsigned column)
{ float result = 0; for (unsigned i = 0; i < 4; ++i) result += point[i] * matrix[i * 4 + column]; return result; }
}
int main()
{
    rsf::skyrim::NativeCamera native{};
    for (unsigned i = 0; i < 4; ++i) native.view[i * 5] = 1;
    native.near_plane = 1; native.far_plane = 101;
    native.projection[0] = 2; native.projection[5] = 3; native.projection[10] = 1.01f;
    native.projection[11] = 1; native.projection[14] = -1.01f;
    std::memcpy(native.view_projection, native.projection, 64);
    std::memcpy(native.previous_view_projection, native.projection, 64);
    native.origin[0] = 10; native.previous_origin[0] = 9;
    native.jitter_ndc[0] = -0.0005f; native.jitter_ndc[1] = 0.001f;
    rsf_camera_frame frame{}; float backward[16]{}; bool history{};
    check(rsf::skyrim::camera_frame(native, 1000, 500, frame, backward, history) && history, "valid perspective and history");
    float jittered_projection[16]{};
    std::memcpy(jittered_projection, native.projection, sizeof(jittered_projection));
    // Native BuildCameraCache postmultiplies projection by clip translation.
    for (unsigned row = 0; row < 4; ++row) {
        jittered_projection[row * 4] += native.projection[row * 4 + 3] * native.jitter_ndc[0];
        jittered_projection[row * 4 + 1] += native.projection[row * 4 + 3] * native.jitter_ndc[1];
    }
    const float sample[]{1, 2, 5, 1};
    const float w = transformed(sample, native.projection, 3);
    const float displacement_x = (transformed(sample, jittered_projection, 0) - transformed(sample, native.projection, 0)) / w * 500;
    const float displacement_y = -(transformed(sample, jittered_projection, 1) - transformed(sample, native.projection, 1)) / w * 250;
    check(std::abs(frame.jitter_pixels[0] - displacement_x) < 1e-5f && std::abs(frame.jitter_pixels[1] - displacement_y) < 1e-5f,
        "reported jitter matches native projected geometry in Y-down pixels");
    const float world[]{10, 0, 5, 1};
    check(std::abs(transformed(world, frame.world_to_view, 0)) < 1e-5f, "absolute world origin conversion");
    const float clip[]{0, 0, 4.04f, 5};
    float previous[4]{}; for (unsigned i = 0; i < 4; ++i) previous[i] = transformed(clip, frame.clip_to_previous_clip, i);
    check(std::abs(previous[0] / previous[3] - 0.4f) < 1e-5f, "one-unit origin shift reprojects to previous camera");
    for (unsigned i = 0; i < 4; ++i) check(std::abs(transformed(previous, backward, i) - clip[i]) < 1e-5f, "bidirectional reprojection roundtrip");
    for (const float z : {1.0f, 101.0f}) {
        const float point[]{0, 0, z, 1};
        const float depth = transformed(point, frame.view_to_clip, 2) / transformed(point, frame.view_to_clip, 3);
        check(std::abs(depth - (z == 1 ? 0 : 1)) < 1e-5f, "standard near/far depth endpoints");
    }
    std::fill_n(native.previous_view_projection, 16, 0.0f);
    check(rsf::skyrim::camera_frame(native, 1000, 500, frame, backward, history) && !history, "empty history resets without rejecting current camera");
    native.view_projection[0] += 1;
    check(!rsf::skyrim::camera_frame(native, 1000, 500, frame, backward, history), "mismatched camera/projection rejected");
    native.view_projection[0] -= 1; native.projection[0] = std::numeric_limits<float>::quiet_NaN();
    check(!rsf::skyrim::camera_frame(native, 1000, 500, frame, backward, history), "nonfinite projection rejected");
    std::printf("Skyrim camera: %d failures\n", failures); return failures ? 1 : 0;
}
