// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>

typedef void (*rsf_anomaly_input_log)(void*, const char*);

typedef struct rsf_anomaly_input_frame {
    void* depth;
    void* motion;
    float clip_to_previous_clip[16];
    uint32_t width, height;
} rsf_anomaly_input_frame;

typedef struct rsf_anomaly_input_desc {
    uint32_t format, width, height, samples, layers, mips;
} rsf_anomaly_input_desc;

/* Borrowed from the Rust owner for one dispatch: the three textures and their views. */
typedef struct rsf_anomaly_input_targets {
    void* depth_copy;
    void* depth;
    void* motion;
    void* depth_view;
    void* depth_uav;
    void* motion_uav;
} rsf_anomaly_input_targets;

typedef struct rsf_anomaly_input_pipeline {
    void* shader;
    void* constants;
} rsf_anomaly_input_pipeline;

typedef struct rsf_anomaly_input_constants {
    float clip_to_previous[16];
    float size[2];
    float has_motion, first_person_limit, validity_threshold, padding[3];
} rsf_anomaly_input_constants;

typedef struct rsf_anomaly_input_platform {
    void* (*device)(void* context);
    void (*release_interface)(void*);
    void (*describe)(void* texture, rsf_anomaly_input_desc*);
    uint32_t (*same_device)(void* texture, void* device);
    void* (*shader)(void* device, rsf_anomaly_input_log, void*);
    void* (*constant_buffer)(void* device);
    /* Each create returns one reference released with release_interface, or null. The format
       is a DXGI_FORMAT and bind a D3D11_BIND_FLAG mask for a single-sample, single-mip texture. */
    void* (*texture)(void* device, uint32_t width, uint32_t height, uint32_t format, uint32_t bind);
    void* (*depth_view)(void* device, void* depth_copy);
    void* (*unordered_view)(void* device, void* texture);
    void* (*motion_view)(void* device, void* texture);
    uint32_t (*dispatch)(void* context, const rsf_anomaly_input_pipeline* pipeline,
        const rsf_anomaly_input_targets* targets, void* depth,
        void* motion_view, const rsf_anomaly_input_constants*, uint32_t groups_x, uint32_t groups_y);
    float first_person_limit, validity_threshold;
} rsf_anomaly_input_platform;

#ifdef __cplusplus
static_assert(sizeof(rsf_anomaly_input_targets) == 48 && sizeof(rsf_anomaly_input_platform) == 96);
namespace rsf::anomaly::fg {
const rsf_anomaly_input_platform& input_platform() noexcept;
}
#endif
