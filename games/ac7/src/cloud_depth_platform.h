// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stdint.h>

typedef struct rsf_ac7_cloud_depth {
    void* device;
    void* shader;
    void* constants;
    void* texture;
    void* view;
    uint32_t width, height;
    bool refused;
} rsf_ac7_cloud_depth;

typedef struct rsf_ac7_cloud_depth_facts {
    float viewport_width, viewport_height;
    uint32_t far_width, far_height, grid_width, grid_height, hdr_bytes;
} rsf_ac7_cloud_depth_facts;

typedef struct rsf_ac7_cloud_depth_params {
    uint32_t size[2], divisor[2], grid[2];
    float minimum_km, minimum_opacity;
} rsf_ac7_cloud_depth_params;

typedef struct rsf_ac7_cloud_depth_leaves {
    void (*retain)(void*);
    void (*release)(void*);
    bool (*create_pipeline)(void* device, void** shader, void** constants);
    bool (*create_target)(void* device, uint32_t width, uint32_t height, void** texture, void** view);
} rsf_ac7_cloud_depth_leaves;

#ifdef __cplusplus
static_assert(sizeof(rsf_ac7_cloud_depth) == 56 && sizeof(rsf_ac7_cloud_depth_facts) == 28);
static_assert(sizeof(rsf_ac7_cloud_depth_params) == 32 && sizeof(rsf_ac7_cloud_depth_leaves) == 32);
extern "C" {
#endif
/* The renderer exclusively lends its initialized state and the current device. */
bool rsf_core_ac7_cloud_depth_prepare(rsf_ac7_cloud_depth*, void* device,
    const rsf_ac7_cloud_depth_facts*, rsf_ac7_cloud_depth_params*, const rsf_ac7_cloud_depth_leaves*);
void rsf_core_ac7_cloud_depth_release(rsf_ac7_cloud_depth*, const rsf_ac7_cloud_depth_leaves*);
#ifdef __cplusplus
}
#endif
