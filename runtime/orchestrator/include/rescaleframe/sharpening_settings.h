/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
#include <stdint.h>
#define RSF_SHARPENING_ABI_VERSION 1u
#define RSF_SHARPENING_OFF 0u
#define RSF_SHARPENING_RCAS 1u
#define RSF_SHARPENING_DEPTH_RCAS 2u
#define RSF_SHARPENING_DEPTH_DAS 3u
#define RSF_SHARPENING_VENDOR 4u
/* Independent of backend selection. Vendor mode is available only for FFX. */
typedef struct rsf_sharpening_options {
    uint32_t struct_size, abi_version;
    uint32_t mode;
    float sharpness;
    uint32_t mas;
    float motion_sharpness;
    float motion_threshold;
    float motion_scale_limit;
    uint32_t contrast_enabled;
    float contrast;
    float depth_scale;
    float depth_bias;
    uint32_t clamp_output;
    uint32_t debug;
} rsf_sharpening_options;
static inline rsf_sharpening_options rsf_sharpening_defaults(void) {
    rsf_sharpening_options o = {sizeof(rsf_sharpening_options), RSF_SHARPENING_ABI_VERSION,
    0, 0.4f, 0, 0.2f, 0, 10, 0, 0, 35, 0.001f, 0, 0};
    return o;
}
#ifdef __cplusplus
#include <algorithm>
#include <cmath>
namespace rsf::sharpening_settings {
inline float vendor_sharpness(const rsf_sharpening_options& o, uint32_t backend) {
    return o.mode == RSF_SHARPENING_VENDOR && backend >= 2 && backend <= 4 ? o.sharpness : 0.0f;
}
inline rsf_sharpening_options sanitize(rsf_sharpening_options o) {
    const auto defaults = rsf_sharpening_defaults();
    o.struct_size = sizeof(o); o.abi_version = RSF_SHARPENING_ABI_VERSION;
    if (o.mode > 4) o.mode = defaults.mode;
    o.sharpness = std::isfinite(o.sharpness) ? std::clamp(o.sharpness, 0.0f, 1.0f) : defaults.sharpness;
    if (o.mas > 1) o.mas = defaults.mas;
    o.motion_sharpness = std::isfinite(o.motion_sharpness) ? std::clamp(o.motion_sharpness, -1.0f, 1.0f) : defaults.motion_sharpness;
    o.motion_threshold = std::isfinite(o.motion_threshold) ? std::clamp(o.motion_threshold, 0.0f, 100.0f) : defaults.motion_threshold;
    o.motion_scale_limit = std::isfinite(o.motion_scale_limit) ? std::clamp(o.motion_scale_limit, 0.01f, 100.0f) : defaults.motion_scale_limit;
    if (o.contrast_enabled > 1) o.contrast_enabled = defaults.contrast_enabled;
    o.contrast = std::isfinite(o.contrast) ? std::clamp(o.contrast, 0.0f, 2.0f) : defaults.contrast;
    o.depth_scale = std::isfinite(o.depth_scale) ? std::clamp(o.depth_scale, 0.0f, 600.0f) : defaults.depth_scale;
    o.depth_bias = std::isfinite(o.depth_bias) ? std::clamp(o.depth_bias, 0.0f, 1.0f) : defaults.depth_bias;
    if (o.clamp_output > 1) o.clamp_output = defaults.clamp_output;
    if (o.debug > 1) o.debug = defaults.debug;
    return o;
}
inline bool valid(const rsf_sharpening_options& o) {
    return o.struct_size >= sizeof(o) && o.abi_version == RSF_SHARPENING_ABI_VERSION &&
        o.mode <= 4 &&
        std::isfinite(o.sharpness) && o.sharpness >= 0 && o.sharpness <= 1 &&
        o.mas <= 1 &&
        std::isfinite(o.motion_sharpness) && o.motion_sharpness >= -1 && o.motion_sharpness <= 1 &&
        std::isfinite(o.motion_threshold) && o.motion_threshold >= 0 && o.motion_threshold <= 100 &&
        std::isfinite(o.motion_scale_limit) && o.motion_scale_limit >= 0.01f && o.motion_scale_limit <= 100 &&
        o.contrast_enabled <= 1 &&
        std::isfinite(o.contrast) && o.contrast >= 0 && o.contrast <= 2 &&
        std::isfinite(o.depth_scale) && o.depth_scale >= 0 && o.depth_scale <= 600 &&
        std::isfinite(o.depth_bias) && o.depth_bias >= 0 && o.depth_bias <= 1 &&
        o.clamp_output <= 1 &&
        o.debug <= 1;
}
}
#endif
