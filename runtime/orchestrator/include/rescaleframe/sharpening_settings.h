/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>
#define RSF_SHARPENING_ABI_VERSION 1u
#define RSF_SHARPENING_OFF 0u
#define RSF_SHARPENING_RCAS 1u
#define RSF_SHARPENING_DEPTH_RCAS 2u
#define RSF_SHARPENING_DEPTH_DAS 3u
#define RSF_SHARPENING_VENDOR 4u
#define RSF_SHARPENING_DEFAULT_MODE 0u
#define RSF_SHARPENING_DEFAULT_SHARPNESS 0.4f
#define RSF_SHARPENING_DEFAULT_MAS 0u
#define RSF_SHARPENING_DEFAULT_MOTION_SHARPNESS 0.2f
#define RSF_SHARPENING_DEFAULT_MOTION_THRESHOLD 0.0f
#define RSF_SHARPENING_DEFAULT_MOTION_SCALE_LIMIT 10.0f
#define RSF_SHARPENING_DEFAULT_CONTRAST_ENABLED 0u
#define RSF_SHARPENING_DEFAULT_CONTRAST 0.0f
#define RSF_SHARPENING_DEFAULT_DEPTH_SCALE 35.0f
#define RSF_SHARPENING_DEFAULT_DEPTH_BIAS 0.001f
#define RSF_SHARPENING_DEFAULT_CLAMP_OUTPUT 0u
#define RSF_SHARPENING_DEFAULT_DEBUG 0u
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
#ifdef __cplusplus
extern "C" {
#endif
rsf_sharpening_options rsf_sharpening_core_defaults(void);
uint32_t rsf_sharpening_core_valid(rsf_sharpening_options options);
#ifdef __cplusplus
}
#endif
#ifdef RSF_RUNTIME_BUILD
static inline rsf_sharpening_options rsf_sharpening_defaults(void) {
    return rsf_sharpening_core_defaults();
}
#else
/* Also used by adapters which build against the header without linking the Runtime. */
static inline rsf_sharpening_options rsf_sharpening_defaults(void) {
    rsf_sharpening_options o = {sizeof(rsf_sharpening_options), RSF_SHARPENING_ABI_VERSION,
        RSF_SHARPENING_DEFAULT_MODE, RSF_SHARPENING_DEFAULT_SHARPNESS,
        RSF_SHARPENING_DEFAULT_MAS, RSF_SHARPENING_DEFAULT_MOTION_SHARPNESS,
        RSF_SHARPENING_DEFAULT_MOTION_THRESHOLD, RSF_SHARPENING_DEFAULT_MOTION_SCALE_LIMIT,
        RSF_SHARPENING_DEFAULT_CONTRAST_ENABLED, RSF_SHARPENING_DEFAULT_CONTRAST,
        RSF_SHARPENING_DEFAULT_DEPTH_SCALE, RSF_SHARPENING_DEFAULT_DEPTH_BIAS,
        RSF_SHARPENING_DEFAULT_CLAMP_OUTPUT, RSF_SHARPENING_DEFAULT_DEBUG};
    return o;
}
#endif
#ifdef __cplusplus
namespace rsf::sharpening_settings {
inline bool valid(const rsf_sharpening_options& o) {
    return o.struct_size >= sizeof(o) && rsf_sharpening_core_valid(o) != 0;
}
}
#endif
