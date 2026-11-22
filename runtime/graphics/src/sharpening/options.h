/* SPDX-License-Identifier: GPL-3.0-only */
/* Constant mapping and MAS ramp ported from OptiScaler 2fff542 (GPL-3.0). */
/* See THIRD_PARTY.md for provenance. */
#pragma once
#include <math.h>
#include <stdint.h>

enum {
    SHARPEN_MODE_OFF,
    SHARPEN_MODE_RCAS,
    SHARPEN_MODE_DEPTH_RCAS,
    SHARPEN_MODE_DEPTH_DAS,
    SHARPEN_MODE_VENDOR
};

/* Internal settings model. Not a DLL contract; hosts must use size/version checked C ABI. */
typedef struct SharpenOptions {
    uint32_t mode;
    float sharpness;
    uint32_t mas;
    float motion_sharpness, motion_threshold, motion_scale_limit;
    uint32_t contrast_enabled;
    float contrast;
    float depth_scale, depth_bias;
    uint32_t clamp_output, debug;
} SharpenOptions;

typedef struct SharpenFrame {
    uint32_t width, height, motion_width, motion_height, depth_width, depth_height;
    uint32_t depth_linear, depth_inverted, hdr, exposure_texture;
    float near_plane, far_plane, exposure, motion_x, motion_y;
} SharpenFrame;

typedef struct SharpenConstants {
    float sharpness, contrast;
    uint32_t mas, display_motion;
    uint32_t debug, motion_width, motion_height;
    float motion_sharpness;
    float motion_scale[2], motion_x, motion_y;
    float threshold, limit;
    uint32_t width, height;
    uint32_t depth_linear, depth_inverted;
    float depth_scale, depth_bias;
    float depth_a, depth_b, depth_c;
    uint32_t clamp_output;
    float depth_texture_scale[2];
    uint32_t depth_width, depth_height;
    uint32_t hdr, exposure_texture, known_planes;
    float exposure;
} SharpenConstants;
_Static_assert(sizeof(SharpenConstants) == 128, "shader constant buffer is 128 bytes");

static inline float sharpen_clamp(float value, float low, float high)
{
    return value < low ? low : (high < value ? high : value);
}

static inline float sharpen_bounded(float value, float low, float high, float fallback)
{
    return isfinite(value) ? sharpen_clamp(value, low, high) : fallback;
}

static inline SharpenOptions sharpen_sanitize(SharpenOptions o)
{
    if (o.mode > SHARPEN_MODE_VENDOR) o.mode = SHARPEN_MODE_OFF;
    o.sharpness = sharpen_bounded(o.sharpness, 0, 1, 0.4f);
    o.mas = o.mas != 0;
    o.contrast_enabled = o.contrast_enabled != 0;
    o.clamp_output = o.clamp_output != 0;
    o.debug = o.debug != 0;
    o.motion_sharpness = sharpen_bounded(o.motion_sharpness, -1, 1, 0.2f);
    o.motion_threshold = sharpen_bounded(o.motion_threshold, 0, 100, 0);
    o.motion_scale_limit = sharpen_bounded(o.motion_scale_limit, 0.01f, 100, 10);
    o.contrast = sharpen_bounded(o.contrast, 0, 2, 0);
    o.depth_scale = sharpen_bounded(o.depth_scale, 0, 600, 35);
    o.depth_bias = sharpen_bounded(o.depth_bias, 0, 1, 0.001f);
    return o;
}

static inline int sharpen_active(const SharpenOptions* o)
{
    return o->mode != SHARPEN_MODE_OFF && o->mode != SHARPEN_MODE_VENDOR &&
        (o->sharpness > 0 || (o->mas && o->motion_sharpness > 0));
}

static inline float sharpen_adaptive(float base, float motion, const SharpenOptions* o, float maximum)
{
    float amount = 0;
    if (o->mas && motion > o->motion_threshold && o->motion_scale_limit > o->motion_threshold) {
        float ramp = (motion - o->motion_threshold) / (o->motion_scale_limit - o->motion_threshold);
        amount = (1.0f < ramp ? 1.0f : ramp) * o->motion_sharpness;
    }
    return sharpen_clamp(base + amount, 0.0f, maximum);
}

static inline SharpenConstants sharpen_constants(
    const SharpenOptions* request, const SharpenFrame* f, uint32_t effective)
{
    const SharpenOptions o = sharpen_sanitize(*request);
    const int depth_mode = effective == SHARPEN_MODE_DEPTH_RCAS || effective == SHARPEN_MODE_DEPTH_DAS;
    SharpenConstants c = {0};
    c.sharpness = o.sharpness * (depth_mode ? 2.0f : 1.0f);
    c.contrast = o.contrast_enabled ? o.contrast : 0;
    c.mas = o.mas && f->motion_width && f->motion_height;
    c.display_motion = f->width == f->motion_width && f->height == f->motion_height;
    c.debug = o.debug ? (effective == SHARPEN_MODE_RCAS ? 1u : 3u) : 0u;
    c.motion_width = f->motion_width;
    c.motion_height = f->motion_height;
    c.motion_sharpness = o.motion_sharpness;
    c.motion_scale[0] = f->width ? (float)f->motion_width / (float)f->width : 0;
    c.motion_scale[1] = f->height ? (float)f->motion_height / (float)f->height : 0;
    c.motion_x = f->motion_x;
    c.motion_y = f->motion_y;
    c.threshold = o.motion_threshold;
    c.limit = o.motion_scale_limit;
    c.width = f->width;
    c.height = f->height;
    c.depth_linear = f->depth_linear;
    c.depth_inverted = f->depth_inverted;
    c.depth_scale = o.depth_scale;
    c.depth_bias = o.depth_bias;
    c.known_planes = isfinite(f->near_plane) && isfinite(f->far_plane) && f->near_plane > 0 &&
        f->far_plane > f->near_plane;
    if (c.known_planes) {
        c.depth_a = f->near_plane * f->far_plane;
        c.depth_b = f->far_plane;
        c.depth_c = f->far_plane - f->near_plane;
    }
    c.clamp_output = depth_mode ? o.clamp_output : 0;
    c.depth_texture_scale[0] = f->width ? (float)f->depth_width / (float)f->width : 0;
    c.depth_texture_scale[1] = f->height ? (float)f->depth_height / (float)f->height : 0;
    c.depth_width = f->depth_width;
    c.depth_height = f->depth_height;
    c.hdr = f->hdr;
    c.exposure_texture = f->exposure_texture;
    c.exposure = sharpen_bounded(f->exposure, 1e-6f, 1e6f, 1);
    return c;
}
