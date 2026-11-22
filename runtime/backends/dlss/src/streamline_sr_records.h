/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>
#include "../../../vulkan/streamline/sdk_records.h"

/* Streamline 2.14.1 records that only the Direct3D SR callers fill. The header, preferences,
   constants, resource and tag records are the ones in sdk_records.h. dlss_streamline.cpp checks
   every record against the pinned SDK declarations; Rust builds them and calls the SDK entry
   points directly. */
typedef struct rsf_sl_sdk_version {
    uint32_t major, minor, build;
} rsf_sl_sdk_version;

typedef struct rsf_sl_sdk_viewport {
    rsf_slvk_sdk_header base;
    uint32_t value;
} rsf_sl_sdk_viewport;

typedef struct rsf_sl_sdk_adapter {
    rsf_slvk_sdk_header base;
    uint8_t* luid;
    uint32_t luid_size;
    void* vk_physical_device;
} rsf_sl_sdk_adapter;

typedef struct rsf_sl_sdk_dlss_options {
    rsf_slvk_sdk_header base;
    uint32_t mode, output_width, output_height;
    float sharpness, pre_exposure, exposure_scale;
    uint8_t hdr, invert_axis_x, invert_axis_y;
    uint32_t presets[6];
    uint8_t auto_exposure, alpha;
} rsf_sl_sdk_dlss_options;

typedef struct rsf_sl_sdk_dlss_optimal {
    rsf_slvk_sdk_header base;
    uint32_t width, height;
    float sharpness;
    uint32_t width_min, height_min, width_max, height_max;
} rsf_sl_sdk_dlss_optimal;

typedef struct rsf_sl_sdk_requirements {
    rsf_slvk_sdk_header base;
    uint32_t flags, max_cpu_threads, max_viewports, num_required_tags;
    const uint32_t* required_tags;
    rsf_sl_sdk_version os_detected, os_required, driver_detected, driver_required;
    uint32_t vk_compute_queues, vk_graphics_queues, vk_num_device_extensions;
    const char** vk_device_extensions;
    uint32_t vk_num_instance_extensions;
    const char** vk_instance_extensions;
    uint32_t vk_num_features12;
    const char** vk_features12;
    uint32_t vk_num_features13;
    const char** vk_features13;
    uint32_t vk_optical_flow_queues;
} rsf_sl_sdk_requirements;
