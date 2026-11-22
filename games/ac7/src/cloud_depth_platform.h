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

#ifdef __cplusplus
static_assert(sizeof(rsf_ac7_cloud_depth) == 56);
extern "C" {
#endif
/* One-to-one depth-bounds kernel. Returns an owned ID3D11ComputeShader. */
void* rsf_ac7_create_truesky_depth_shader(void* device);
/* Borrows the immediate context at composite_tile and returns the state's ID3D11Texture2D.
   The viewport dimensions come from that draw. Returns null when its bindings do not match. */
void* rsf_ac7_cloud_depth_write(rsf_ac7_cloud_depth*, void* context, float width, float height);
void rsf_ac7_cloud_depth_release(rsf_ac7_cloud_depth*);
#ifdef __cplusplus
}
#endif
