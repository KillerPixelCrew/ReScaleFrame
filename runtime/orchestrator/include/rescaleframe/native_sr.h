/* SPDX-License-Identifier: MIT */
#ifndef RSF_NATIVE_SR_H
#define RSF_NATIVE_SR_H
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/game_renderer.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Run on the D3D11 execution owner. The game has queued a complete spatial fallback first.
   On success writes both engine output resources, restores bindings, and retains no game input.
   Native family identity is used for SR history only; it never creates a simulation/FG frame ID. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_native_sr_evaluate(void* d3d11_context,
    const rsf_game_render_pass* pass);
/* Graphics owner, synchronous FG_INPUTS lease before native inputs are modified.
   Normalizes/copies depth and motion into native_fg ownership without SR startup or
   native output writes. Borrows textures only for the call; bindings are restored.
   Currently requires camera-inclusive motion and explicit canonical UV scales. */
RSF_RUNTIME_API int32_t rsf_native_sr_capture_fg(void* context,
    const rsf_game_render_pass*, const rsf_motion_decode_params*);

/* Called on the graphics owner before releasing the pipeline device. */
RSF_RUNTIME_API void rsf_native_sr_release_resources(void);
#ifdef __cplusplus
}
#endif
#endif
