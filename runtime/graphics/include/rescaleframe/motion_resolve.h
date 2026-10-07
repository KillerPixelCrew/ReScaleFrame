/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_MOTION_RESOLVE_H
#define RSF_MOTION_RESOLVE_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Dense current-to-previous pixel motion from sparse decoded vectors and device depth.
   The pass owns its device and fixed-size output textures; no temporal history is stored.
   Serialize runs/access/destruction on the owning immediate-context thread. */
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
/* Retain ID3D11Device and allocate fixed-size targets; recreate to resize. Create/run return 1
   on success and 0 for invalid inputs, compilation/resource failures, or failed state save. */
int rsf_motion_resolve_create(void* d3d11_device, uint32_t width, uint32_t height,
                             rsf_motion_resolve** out);
/* Borrow same-device ID3D11Texture2D inputs covering width/height from origin zero, single-sample
   and non-array. depth_layer uses reversed Z (larger is nearer); incompatible layers are ignored.
   Dispatch restores saved graphics state. Existing written vectors replace camera motion. */
int rsf_motion_resolve_run(rsf_motion_resolve* pass, void* context, void* decoded_motion,
                          void* depth, const rsf_motion_resolve_params* params);
/* Borrowed ID3D11Texture2D outputs, overwritten by each successful run, valid until destroy. */
void* rsf_motion_resolve_motion(rsf_motion_resolve* pass);
void* rsf_motion_resolve_depth(rsf_motion_resolve* pass);
/* Null is accepted. Finish GPU use of the owned outputs before destruction. */
void rsf_motion_resolve_destroy(rsf_motion_resolve* pass);
#ifdef __cplusplus
}
#endif
#endif
