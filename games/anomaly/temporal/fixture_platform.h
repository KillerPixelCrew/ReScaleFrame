/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct rsf_anomaly_plan_matrix { double values[16]; } rsf_anomaly_plan_matrix;
typedef struct rsf_anomaly_plan_extent { uint32_t width, height; } rsf_anomaly_plan_extent;
typedef struct rsf_anomaly_plan_vec2 { double x, y; } rsf_anomaly_plan_vec2;
typedef struct rsf_anomaly_plan_vec4 { double x, y, z, w; } rsf_anomaly_plan_vec4;
typedef struct rsf_anomaly_plan_resolutions {
    rsf_anomaly_plan_extent output, render, pda;
} rsf_anomaly_plan_resolutions;
typedef struct rsf_anomaly_plan_camera {
    rsf_anomaly_plan_matrix view, projection;
    rsf_anomaly_plan_vec2 jitter;
} rsf_anomaly_plan_camera;
typedef struct rsf_anomaly_plan_frame {
    rsf_anomaly_plan_camera world, first_person;
    rsf_anomaly_plan_resolutions sizes;
    uint64_t source_frame;
    uint8_t view;
    bool has_first_person, camera_cut, first_person_cut;
} rsf_anomaly_plan_frame;
typedef struct rsf_anomaly_plan_token {
    uint64_t source_frame, serial, owner;
    uint8_t view;
} rsf_anomaly_plan_token;
typedef struct rsf_anomaly_plan_key { uint64_t owner, visual, generation; } rsf_anomaly_plan_key;
typedef struct rsf_anomaly_plan_pose {
    rsf_anomaly_plan_matrix world;
    uint64_t palette_layout;
    const rsf_anomaly_plan_matrix* bones;
    size_t bone_count;
    uint8_t domain, kind;
} rsf_anomaly_plan_pose;
typedef struct rsf_anomaly_plan_capture {
    rsf_anomaly_plan_matrix previous_world;
    uint32_t status;
    bool has_previous;
} rsf_anomaly_plan_capture;

#ifdef __cplusplus
extern "C" {
#endif
/* The native geometry fixture owns this handle and calls it on its render thread. */
void* rsf_anomaly_plan_history_create(void);
void rsf_anomaly_plan_history_destroy(void*);
uint32_t rsf_anomaly_plan_history_begin(void*, const rsf_anomaly_plan_frame*, rsf_anomaly_plan_token*);
void rsf_anomaly_plan_history_capture(void*, const rsf_anomaly_plan_token*, const rsf_anomaly_plan_key*,
    const rsf_anomaly_plan_pose*, rsf_anomaly_plan_capture*);
bool rsf_anomaly_plan_history_retire(void*, const rsf_anomaly_plan_key*);
#ifdef __cplusplus
}
#endif
