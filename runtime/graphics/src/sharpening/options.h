// SPDX-License-Identifier: GPL-3.0-only
// Constant mapping and MAS ramp ported from OptiScaler 2fff542 (GPL-3.0).
// See THIRD_PARTY.md for provenance.
#pragma once
#include <algorithm>
#include <cmath>
#include <cstdint>
namespace rsf::sharpening {
enum class Mode : uint32_t { off, rcas, depth_rcas, depth_das, vendor };
// Internal settings model. Not a DLL contract; hosts must use size/version checked C ABI.
struct Options {
    Mode mode = Mode::off;
    float sharpness = 0.4f;
    uint32_t mas = 0;
    float motion_sharpness = 0.2f, motion_threshold = 0, motion_scale_limit = 10;
    uint32_t contrast_enabled = 0;
    float contrast = 0;
    float depth_scale = 35, depth_bias = 0.001f;
    uint32_t clamp_output = 0, debug = 0;
};
inline float bounded(float value, float low, float high, float fallback) {
    return std::isfinite(value) ? std::clamp(value,low,high) : fallback;
}
inline Options sanitize(Options o) {
    if (static_cast<uint32_t>(o.mode) > 4) o.mode = Mode::off;
    o.sharpness = bounded(o.sharpness,0,1,0.4f);
    o.mas = o.mas != 0; o.contrast_enabled = o.contrast_enabled != 0;
    o.clamp_output = o.clamp_output != 0; o.debug = o.debug != 0;
    o.motion_sharpness = bounded(o.motion_sharpness,-1,1,0.2f);
    o.motion_threshold = bounded(o.motion_threshold,0,100,0);
    o.motion_scale_limit = bounded(o.motion_scale_limit,0.01f,100,10);
    o.contrast = bounded(o.contrast,0,2,0);
    o.depth_scale = bounded(o.depth_scale,0,600,35);
    o.depth_bias = bounded(o.depth_bias,0,1,0.001f);
    return o;
}
inline bool active(const Options& o) {
    return o.mode != Mode::off && o.mode != Mode::vendor &&
        (o.sharpness > 0 || (o.mas && o.motion_sharpness > 0));
}
inline float adaptive(float base, float motion, const Options& o, float maximum = 1) {
    float amount = 0;
    if (o.mas && motion > o.motion_threshold && o.motion_scale_limit > o.motion_threshold)
        amount = std::min((motion-o.motion_threshold)/(o.motion_scale_limit-o.motion_threshold),1.0f)*o.motion_sharpness;
    return std::clamp(base+amount,0.0f,maximum);
}
struct Frame {
    uint32_t width, height, motion_width, motion_height, depth_width, depth_height;
    uint32_t depth_linear = 0, depth_inverted = 0, hdr = 1, exposure_texture = 0;
    float near_plane = 0, far_plane = 0, exposure = 1, motion_x = 1, motion_y = 1;
};
struct Constants {
    float sharpness, contrast; uint32_t mas, display_motion;
    uint32_t debug, motion_width, motion_height; float motion_sharpness;
    float motion_scale[2], motion_x, motion_y;
    float threshold, limit; uint32_t width, height;
    uint32_t depth_linear, depth_inverted; float depth_scale, depth_bias;
    float depth_a, depth_b, depth_c; uint32_t clamp_output;
    float depth_texture_scale[2]; uint32_t depth_width, depth_height;
    uint32_t hdr, exposure_texture, known_planes; float exposure;
};
static_assert(sizeof(Constants) == 128);
inline Constants constants(const Options& request, const Frame& f, Mode effective) {
    const auto o = sanitize(request);
    Constants c{};
    c.sharpness = o.sharpness * ((effective == Mode::depth_rcas || effective == Mode::depth_das) ? 2.0f : 1.0f);
    c.contrast = o.contrast_enabled ? o.contrast : 0;
    c.mas = o.mas && f.motion_width && f.motion_height;
    c.display_motion = f.width == f.motion_width && f.height == f.motion_height;
    c.debug = o.debug ? (effective == Mode::rcas ? 1u : 3u) : 0u;
    c.motion_width = f.motion_width; c.motion_height = f.motion_height; c.motion_sharpness = o.motion_sharpness;
    c.motion_scale[0] = f.width ? float(f.motion_width)/float(f.width) : 0;
    c.motion_scale[1] = f.height ? float(f.motion_height)/float(f.height) : 0;
    c.motion_x = f.motion_x; c.motion_y = f.motion_y;
    c.threshold = o.motion_threshold; c.limit = o.motion_scale_limit;
    c.width = f.width; c.height = f.height;
    c.depth_linear = f.depth_linear; c.depth_inverted = f.depth_inverted;
    c.depth_scale = o.depth_scale; c.depth_bias = o.depth_bias;
    c.known_planes = std::isfinite(f.near_plane) && std::isfinite(f.far_plane) && f.near_plane > 0 && f.far_plane > f.near_plane;
    if (c.known_planes) { c.depth_a = f.near_plane*f.far_plane; c.depth_b = f.far_plane; c.depth_c = f.far_plane-f.near_plane; }
    c.clamp_output = effective == Mode::depth_rcas || effective == Mode::depth_das ? o.clamp_output : 0;
    c.depth_texture_scale[0] = f.width ? float(f.depth_width)/float(f.width) : 0;
    c.depth_texture_scale[1] = f.height ? float(f.depth_height)/float(f.height) : 0;
    c.depth_width = f.depth_width; c.depth_height = f.depth_height;
    c.hdr = f.hdr; c.exposure_texture = f.exposure_texture; c.exposure = bounded(f.exposure,1e-6f,1e6f,1);
    return c;
}
}
