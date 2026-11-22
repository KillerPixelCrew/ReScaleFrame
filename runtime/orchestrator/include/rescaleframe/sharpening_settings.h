/* SPDX-License-Identifier: MIT */
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
// Static Runtime linkage for the in-tree C++ settings consumers.
extern "C" {
void rsf_core_sharpening_sanitize(rsf_sharpening_options*, const rsf_sharpening_options* defaults);
uint32_t rsf_core_sharpening_valid(const rsf_sharpening_options*, const rsf_sharpening_options* defaults);
float rsf_core_sharpening_vendor_amount(const rsf_sharpening_options*, uint32_t backend);
}

namespace rsf::sharpening_settings {
inline float vendor_sharpness(const rsf_sharpening_options& options, uint32_t backend) {
    return rsf_core_sharpening_vendor_amount(&options, backend);
}
inline rsf_sharpening_options sanitize(rsf_sharpening_options options) {
    const auto defaults = rsf_sharpening_defaults();
    rsf_core_sharpening_sanitize(&options, &defaults);
    return options;
}
inline bool valid(const rsf_sharpening_options& options) {
    const auto defaults = rsf_sharpening_defaults();
    return rsf_core_sharpening_valid(&options, &defaults) != 0;
}
}
#endif
