/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_MOTION_RESOLVE_H
#define RSF_MOTION_RESOLVE_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_motion_resolve rsf_motion_resolve;
/* Resolve an already-decoded sparse field. Written vectors replace depth-derived camera motion;
   they are not added. The plugin supplies their direction and units through decoded_to_pixels.
   Matrix is row-major current clip to previous clip, with jitter removed. Outputs are dense
   R16G16_FLOAT pixel displacement and R32_FLOAT device depth. Origin-zero valid rectangle. */
typedef struct rsf_motion_resolve_params {
    uint32_t struct_size;
    float clip_to_previous[16];
    float decoded_to_pixels[2];
    float sentinel;
    uint32_t has_sentinel;
    /* Optional R32_FLOAT device-depth layer, origin-zero like the inputs (volumetric clouds).
       Unwritten pixels reproject at the nearer of it and scene depth. The depth output stays the
       scene depth. Null for none. */
    void* depth_layer;
} rsf_motion_resolve_params;
int rsf_motion_resolve_create(void* d3d11_device, uint32_t width, uint32_t height,
                             rsf_motion_resolve** out);
int rsf_motion_resolve_run(rsf_motion_resolve* pass, void* context, void* decoded_motion,
                          void* depth, const rsf_motion_resolve_params* params);
void* rsf_motion_resolve_motion(rsf_motion_resolve* pass);
void* rsf_motion_resolve_depth(rsf_motion_resolve* pass);
void rsf_motion_resolve_destroy(rsf_motion_resolve* pass);
#ifdef __cplusplus
}
#endif
#endif
