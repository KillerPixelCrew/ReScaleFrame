/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_DEPTH_REPLAY_H
#define RSF_DEPTH_REPLAY_H
#include <rescaleframe/frame_tap.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_depth_replay rsf_depth_replay;
/* Device-only setup, outside the render loop. Single-sample R32G8X24_TYPELESS depth, reversed Z.
   No allocation, compilation, or buffer retention for deferred replay occurs in draw/end_frame.
   The owner serializes setup/destruction with callbacks and keeps the object for the session. */
rsf_depth_replay* rsf_depth_replay_create(void* device, uint32_t width, uint32_t height);

/* Last rejection code, or zero before any rejection. A successful replay does not reset it:
     1 missing report/context/depth/target
     2 extent or sample-count mismatch (skip this draw)
     3 unsupported draw kind/topology or missing shader/elements/instances (skip this draw)
     4 unsupported depth-view format/dimension/mip or missing read-only depth flag
     5 depth resource is not Texture2D
     6 incompatible depth texture descriptor
     7 layer, source depth, or context differs from the current frame's candidate
     8 unsupported shader stages/SO/OM UAVs or incompatible depth/stencil state
     9 viewport is not the full origin-zero replay extent with depth range 0..1
   Codes 4..9 refuse the candidate until end_frame; partial replay is then never selected. */
uint32_t rsf_depth_replay_last_reject(const rsf_depth_replay* replay);

/* Last inspected descriptor fields. Fields not reached by the latest candidate retain earlier
   diagnostics; the getter appends the current candidate identities/draw/refusal state. */
typedef struct rsf_depth_replay_detail {
    uint32_t draw_width, draw_height, draw_samples;
    uint32_t dsv_format, dsv_dimension, dsv_flags;
    uint32_t source_width, source_height, source_format, source_samples;
    /* Current candidate identities. layer/source are retained until end_frame; context is borrowed. */
    void* layer;
    void* source;
    void* context;
    uint32_t draws;
    uint32_t refused;
} rsf_depth_replay_detail;

void rsf_depth_replay_get_detail(const rsf_depth_replay* replay, rsf_depth_replay_detail* out);
void rsf_depth_replay_destroy(rsf_depth_replay* replay);
/* Call only synchronously inside the tap's geometry callback, for game-selected candidates.
   The tap suppresses reentry. Returns 1 for a replay, 0 for a skipped draw or refused candidate.
   Sticky codes 4..9 invalidate partial geometry; codes 1..3 permit later eligible draws. */
uint32_t rsf_depth_replay_draw(rsf_depth_replay* replay, const rsf_frame_tap_geometry* draw);
/* Borrowed texture or original depth. Requires same-frame matching layer, source depth, context,
   and at least one successful draw. A missing layer always returns original_depth unchanged. */
void* rsf_depth_replay_selected(rsf_depth_replay* replay, void* context, void* original_depth,
                                void* composed_layer);
/* Release retained source/layer identities and clear this frame's draw/refusal state. The last
   diagnostic rejection/detail remain available; call once per source frame on the render thread. */
void rsf_depth_replay_end_frame(rsf_depth_replay* replay);
#ifdef __cplusplus
}
#endif
#endif
