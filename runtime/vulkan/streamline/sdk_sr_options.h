// SPDX-License-Identifier: MIT
#pragma once
#include "sdk_records.h"

/* sl::DLSSOptions v3. Rust packs the versioned header and SDK defaults. */
typedef struct rsf_vkdlss_options {
    rsf_slvk_sdk_header base;
    uint32_t mode, width, height;
    float sharpness, pre_exposure, exposure_scale;
    uint8_t hdr, invert_x, invert_y;
    uint32_t presets[6];
    uint8_t auto_exposure, alpha_upscaling;
} rsf_vkdlss_options;

/* sl::DLSSOptimalSettings, written by slDLSSGetOptimalSettings. */
typedef struct rsf_vkdlss_optimal {
    rsf_slvk_sdk_header base;
    uint32_t width, height;
    float sharpness;
    uint32_t min_width, min_height, max_width, max_height;
} rsf_vkdlss_optimal;
