/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_NATIVE_TRANSLUCENCY_H
#define RSF_NATIVE_TRANSLUCENCY_H
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/game_renderer.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Hints derived from the game's translucency passes, rect-local at render size (origin zero).
   Owned by the runtime and valid until the next TRANSLUCENCY scope or release. */
typedef struct rsf_native_translucency_masks {
    uint32_t struct_size;
    /* Scene colour as the first translucency pass began: opaque geometry, sky and clouds. */
    void* color_before_transparency;
    /* The offscreen translucency layer (RGB premultiplied, alpha = transmittance), when the game
       drew one at scene size. It is already composited into the SR input. Null otherwise. */
    void* transparency_layer;
    /* R32_FLOAT. Visible change from translucency in a tonemapped space, capped at 0.9. */
    void* reactive;
    /* R32_FLOAT. Translucent coverage in [0,1], including transmittance of the layer. */
    void* coverage;
    /* R32_FLOAT. 1 where coverage reaches one half, else 0. */
    void* bias;
    /* R32_FLOAT device depth of volumetric clouds composited before translucency, zero where
       none qualifies. Only for reprojecting pixels without object velocity. Null when absent. */
    void* motion_depth;
} rsf_native_translucency_masks;

/* Called on the D3D11 execution owner for TRANSLUCENCY, CLOUD_DEPTH and MATERIALS passes.
   MATERIALS/translucency scopes enable sampler mip bias and restore zero at end. The first
   direct translucency pair establishes the opaque snapshot and masks; layer passes merge
   their transmittance coverage. Failure drops hints rather than authorizing stale textures. */
RSF_RUNTIME_API void rsf_native_translucency_pass(void* d3d11_context, const rsf_game_render_pass* pass,
    uint32_t begin);
/* Masks only for the same view, native frame and render rectangle, after the first pass ended. */
RSF_RUNTIME_API int rsf_native_translucency_take(const rsf_game_render_pass* sr_pass, rsf_native_translucency_masks* out);
/* Release cached textures on the graphics owner after all borrowers finish. */
RSF_RUNTIME_API void rsf_native_translucency_release(void);
#ifdef __cplusplus
}
#endif
#endif
