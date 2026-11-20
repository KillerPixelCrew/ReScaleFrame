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

typedef struct rsf_anomaly_input_targets {
    void* resources;
    void* depth;
    void* motion;
} rsf_anomaly_input_targets;

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
    void* (*build)(void* device, rsf_anomaly_input_log, void*);
    void (*release_pipeline)(void*);
    void (*targets)(void* device, uint32_t width, uint32_t height, rsf_anomaly_input_targets*);
    void (*release_targets)(void*);
    void* (*motion_view)(void* device, void* texture);
    uint32_t (*dispatch)(void* context, void* pipeline, void* resources, void* depth,
        void* motion_view, const rsf_anomaly_input_constants*, uint32_t groups_x, uint32_t groups_y);
    float first_person_limit, validity_threshold;
} rsf_anomaly_input_platform;

typedef struct rsf_anomaly_input_owner rsf_anomaly_input_owner;
#ifdef __cplusplus
extern "C" {
#endif
uint32_t rsf_anomaly_input_run(rsf_anomaly_input_owner**, const rsf_anomaly_input_platform*,
    void*, const rsf_anomaly_input_frame*, rsf_anomaly_input_log, void*);
void* rsf_anomaly_input_output(const rsf_anomaly_input_owner*, uint32_t motion);
void rsf_anomaly_input_release(rsf_anomaly_input_owner*);
#ifdef __cplusplus
}
#endif
