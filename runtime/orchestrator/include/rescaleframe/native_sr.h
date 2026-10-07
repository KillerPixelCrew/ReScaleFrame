/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_NATIVE_SR_H
#define RSF_NATIVE_SR_H
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/game_renderer.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Run on the D3D11 execution owner. The game has queued a complete spatial fallback first.
   On success queues writes to both engine output resources and restores bindings. Game frame
   inputs remain borrowed; a two-entry exposure view cache may retain source COM references
   until replacement or release.
   Native family identity is used for SR history only; it never creates a simulation/FG frame ID. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_native_sr_evaluate(void* d3d11_context,
    const rsf_game_render_pass* pass);
/* Frontends set intent; the engine reads one runtime-owned configuration snapshot. */
RSF_RUNTIME_API void rsf_native_sr_set_enabled(uint32_t enabled);
/* Publish the whole engine surface after output/quality selection, independently of a cropped
   backend evaluation extent. Called by the graphics owner; engine readers never query a crop. */
RSF_RUNTIME_API int rsf_native_sr_set_surface(uint32_t width, uint32_t height);
/* Thread-safe host callback. Copy the last published dimensions plus current enabled intent;
   returns zero for short output storage. user is unused. Enablement does not prove evaluation. */
RSF_RUNTIME_API int rsf_native_sr_render_config(void* user, rsf_game_render_config* config);
/* Called on the graphics owner after CPU producers and snapshot consumers stop, before releasing
   the pipeline device. Clears history, composition/CPU/window/scene caches and published intent. */
RSF_RUNTIME_API void rsf_native_sr_release_resources(void);
#ifdef __cplusplus
}
#endif
#endif
