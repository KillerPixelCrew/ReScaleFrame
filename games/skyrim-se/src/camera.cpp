// SPDX-License-Identifier: GPL-3.0-only
#include "camera.h"
#include <algorithm>
#include <cmath>
#include <cstring>

namespace rsf::skyrim {
namespace {
void identity(float* out) { std::fill_n(out, 16, 0.0f); for (unsigned i = 0; i < 4; ++i) out[i * 5] = 1; }
void multiply(const float* a, const float* b, float* out)
{
    float result[16]{};
    for (unsigned r = 0; r < 4; ++r) for (unsigned c = 0; c < 4; ++c) {
        double sum = 0;
        for (unsigned k = 0; k < 4; ++k) sum += double(a[r * 4 + k]) * b[k * 4 + c];
        result[r * 4 + c] = float(sum);
    }
    std::memcpy(out, result, sizeof(result));
}
bool inverse(const float* source, float* out)
{
    double rows[4][8]{};
    for (unsigned r = 0; r < 4; ++r) for (unsigned c = 0; c < 4; ++c) {
        if (!std::isfinite(source[r * 4 + c])) return false;
        rows[r][c] = source[r * 4 + c]; rows[r][c + 4] = r == c ? 1 : 0;
    }
    for (unsigned c = 0; c < 4; ++c) {
        unsigned pivot = c;
        for (unsigned r = c + 1; r < 4; ++r) if (std::abs(rows[r][c]) > std::abs(rows[pivot][c])) pivot = r;
        if (std::abs(rows[pivot][c]) < 1e-12) return false;
        for (unsigned k = 0; k < 8; ++k) std::swap(rows[c][k], rows[pivot][k]);
        const double divisor = rows[c][c];
        for (double& value : rows[c]) value /= divisor;
        for (unsigned r = 0; r < 4; ++r) if (r != c) {
            const double factor = rows[r][c];
            for (unsigned k = 0; k < 8; ++k) rows[r][k] -= rows[c][k] * factor;
        }
    }
    for (unsigned r = 0; r < 4; ++r) for (unsigned c = 0; c < 4; ++c) {
        out[r * 4 + c] = float(rows[r][c + 4]);
        if (!std::isfinite(out[r * 4 + c])) return false;
    }
    return true;
}
bool close(float a, float b) { return std::isfinite(a) && std::isfinite(b) && std::abs(a - b) <= 0.0002f * std::max(1.0f, std::max(std::abs(a), std::abs(b))); }
}
bool camera_frame(const NativeCamera& native, uint32_t width, uint32_t height,
    rsf_camera_frame& out, float previous_clip_to_clip[16], bool& history_valid)
{
    history_valid = false; out = {};
    if (!width || !height || !std::isfinite(native.near_plane) || !std::isfinite(native.far_plane) ||
        native.near_plane <= 0 || native.far_plane <= native.near_plane ||
        native.projection[0] <= 0 || native.projection[5] <= 0 ||
        !close(native.projection[11], 1) || !close(native.projection[15], 0)) return false;
    const float q = native.far_plane / (native.far_plane - native.near_plane);
    if (!close(native.projection[10], q) || !close(native.projection[14], -q * native.near_plane)) return false;
    float product[16]{}, inverse_vp[16]{}, translation[16]{};
    multiply(native.view, native.projection, product);
    for (unsigned i = 0; i < 16; ++i) if (!close(product[i], native.view_projection[i])) return false;
    if (!inverse(native.projection, out.clip_to_view) || !inverse(native.view_projection, inverse_vp)) return false;
    identity(translation);
    for (unsigned i = 0; i < 3; ++i) {
        if (!std::isfinite(native.origin[i]) || !std::isfinite(native.previous_origin[i])) return false;
        translation[12 + i] = -native.origin[i];
    }
    multiply(translation, native.view, out.world_to_view);
    if (!inverse(out.world_to_view, out.view_to_world)) return false;
    identity(translation);
    for (unsigned i = 0; i < 3; ++i) translation[12 + i] = native.origin[i] - native.previous_origin[i];
    float adjusted[16]{};
    multiply(inverse_vp, translation, adjusted);
    multiply(adjusted, native.previous_view_projection, out.clip_to_previous_clip);
    history_valid = inverse(out.clip_to_previous_clip, previous_clip_to_clip);
    if (!history_valid) { identity(out.clip_to_previous_clip); identity(previous_clip_to_clip); }
    out.struct_size = sizeof(out); out.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    std::memcpy(out.view_to_clip, native.projection, sizeof(native.projection));
    // The native cache postmultiplies projection by an NDC translation.
    // Report geometry displacement in pixel coordinates (Y down), as SR expects.
    out.jitter_pixels[0] = native.jitter_ndc[0] * float(width) * 0.5f;
    out.jitter_pixels[1] = -native.jitter_ndc[1] * float(height) * 0.5f;
    if (!std::isfinite(out.jitter_pixels[0]) || !std::isfinite(out.jitter_pixels[1])) return false;
    out.render_width = out.output_width = width; out.render_height = out.output_height = height;
    out.near_plane = native.near_plane; out.far_plane = native.far_plane;
    out.vertical_fov_radians = 2 * std::atan(1 / native.projection[5]);
    out.depth_inverted = out.motion_jittered = 0;
    return true;
}
}
