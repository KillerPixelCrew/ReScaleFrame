// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_fg_vulkan.h>

#define RSF_DLSSGVK_ABI_VERSION 1u
typedef struct rsf_dlssgvk rsf_dlssgvk;
typedef struct rsf_dlssgvk_status {
    uint32_t struct_size, abi_version;
    rsf_fgvk_status presentation;
    uint32_t requested_generated_frames, max_generated_frames;
    uint32_t sdk_status, min_dimension, vsync_supported, dynamic_mfg_supported;
    int32_t sdk_result, asynchronous_present_result;
} rsf_dlssgvk_status;

