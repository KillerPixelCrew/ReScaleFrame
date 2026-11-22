// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <stdint.h>
#include "motion_device_platform.h"
#ifndef __cplusplus
#include <stdbool.h>
#endif

enum {
    RSF_ANOMALY_MOTION_ROOTS,
    RSF_ANOMALY_MOTION_FOLIAGE_LAYOUTS,
    RSF_ANOMALY_MOTION_CORE_DRAW,
    RSF_ANOMALY_MOTION_TREE_DRAW,
    RSF_ANOMALY_MOTION_DETAIL_DRAW,
};

typedef struct rsf_anomaly_motion_far_case {
    const char* vertex;
    const char* pixel;
    const char* element;
    int32_t skin;
} rsf_anomaly_motion_far_case;

typedef struct rsf_anomaly_motion_tess_case {
    const char* name;
    int32_t skin;
    const char* const* defines;
    size_t define_count;
} rsf_anomaly_motion_tess_case;

typedef bool (*rsf_anomaly_motion_core_case_fn)(const void*, uint32_t);
typedef bool (*rsf_anomaly_motion_far_case_fn)(const rsf_anomaly_motion_far_case*, double*);
typedef bool (*rsf_anomaly_motion_tess_case_fn)(const rsf_anomaly_motion_tess_case*, const char* create, bool diagnose);
typedef struct rsf_anomaly_shader_reflection rsf_anomaly_shader_reflection;

#ifdef __cplusplus
extern "C" {
#endif
int32_t rsf_anomaly_motion_core_schedule(const void*, rsf_anomaly_motion_core_case_fn);
int32_t rsf_anomaly_motion_far_schedule(rsf_anomaly_motion_far_case_fn, uint32_t* pairs, uint32_t* lq, double* worst);
int32_t rsf_anomaly_motion_tess_schedule(rsf_anomaly_motion_tess_case_fn, bool include_hm, bool diagnose, const char* create);
// Returns a static refusal message or null after uploading the fixture's buffers.
const char* rsf_anomaly_motion_model_upload(rsf_anomaly_motion_device_owner*, void* context, const double* view,
    const double* projection, const double* previous_bone, bool history, bool tessellated, int32_t skin, uint32_t extent);
const char* rsf_anomaly_motion_foliage_fixture_layout(const rsf_anomaly_shader_reflection*, uint8_t family,
    bool check_fields, bool still);
// Each input batch has two groups of 16 floats. The output rows contain all 61 groups.
const char* rsf_anomaly_motion_detail_fixture_bytes(uint32_t extent, bool include_b, const void* before,
    const void* current, const float* previous_groups, const float* current_groups, float* rows, void* bytes);
#ifdef __cplusplus
}
#endif
