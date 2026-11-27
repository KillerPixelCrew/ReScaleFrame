// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>

#define RSF_XESS_BRIDGE_ALLOCATOR_COUNT 3u

/* Resource description fields needed by the capture owner and its receipts. */
typedef struct rsf_xess_capture_desc {
    uint64_t width;
    uint32_t height, dimension, format, depth_or_array_size, mip_levels, sample_count;
} rsf_xess_capture_desc;

typedef struct rsf_xess_bridge_resource_desc {
    uint64_t width;
    uint32_t height, format;
} rsf_xess_bridge_resource_desc;

typedef struct rsf_xess_bridge_modules {
    void* fg;
    void* xell;
} rsf_xess_bridge_modules;

typedef struct rsf_xess_bridge_shared_image_desc {
    uint32_t width, height, format, flags;
} rsf_xess_bridge_shared_image_desc;
