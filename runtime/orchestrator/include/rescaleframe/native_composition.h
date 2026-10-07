/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_NATIVE_COMPOSITION_H
#define RSF_NATIVE_COMPOSITION_H
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_renderer.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_native_composition_frame {
    uint32_t struct_size;
    uint32_t complete;
    uint64_t session_id;
    uint64_t family_key;
    uint64_t view_key;
    uint64_t native_frame;
    uint64_t scope_id;
    uint32_t resource_generation;
    int32_t output_rect[4];
    /* Runtime-owned snapshots, borrowed until the next capture or release. These identify the
       engine UI composition boundary, not the final presentation or simulation/input frame. */
    void* scene;
    void* ui_raster;
    void* composed;
    uint64_t source_frame_id;
    uint64_t submission_id;
    uint32_t flags;
    uint64_t viewport_key;
} rsf_native_composition_frame;
/* All calls, including reads, run on the graphics execution owner. Capture is demand-driven:
   zero disables, UINT32_MAX enables continuous capture, otherwise capture that many scopes. */
RSF_RUNTIME_API void rsf_native_composition_request(uint32_t scopes);
/* Every consumer must match session/view/native frame; snapshots do not establish input identity. */
RSF_RUNTIME_API int rsf_native_composition_capture(void* context,
    const rsf_game_render_pass* pass, uint32_t begin);
/* Copy the last complete pair into size-initialized output. Captures are same-device, single
   sample, single mip/layer D3D11 textures; copies are queued, without a CPU completion wait. */
RSF_RUNTIME_API int rsf_native_composition_read(rsf_native_composition_frame* frame);
/* Discard snapshots and requests on the graphics owner after consumers have stopped. */
RSF_RUNTIME_API void rsf_native_composition_release(void);
#ifdef __cplusplus
}
#endif
#endif
