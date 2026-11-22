// SPDX-License-Identifier: MIT
#pragma once
#include "sdk_records.h"

/* DLSSGOptions v5. The SDK call adapter installs the native error callback
   after copying this record into a constructed sl::DLSSGOptions. */
typedef struct rsf_slvk_sdk_fg_options {
    rsf_slvk_sdk_header base;
    uint32_t mode, generated, flags;
    uint32_t dynamic_width, dynamic_height, buffers;
    uint32_t guide_width, guide_height, color_width, color_height;
    uint32_t color_format, motion_format, depth_format, hudless_format, ui_format;
    void (*reserved_callback)(void);
    uint8_t reserved_boolean;
    uint32_t queue_parallelism;
    uint8_t ui_recomposition;
    float target_rate;
} rsf_slvk_sdk_fg_options;
