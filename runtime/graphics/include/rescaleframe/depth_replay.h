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
void rsf_depth_replay_destroy(rsf_depth_replay* replay);
/* Call only synchronously inside the tap's geometry callback, for game-selected candidates.
   The tap suppresses reentry. Returns 1 for a replay, 0 for refusal. Unsupported draws poison
   this candidate for the frame so partial geometry cannot silently become backend depth. */
uint32_t rsf_depth_replay_draw(rsf_depth_replay* replay, const rsf_frame_tap_geometry* draw);
/* Borrowed texture or original depth. Requires same-frame matching layer, source depth, context,
   and at least one successful draw. A missing layer always returns original_depth unchanged. */
void* rsf_depth_replay_selected(rsf_depth_replay* replay, void* context, void* original_depth,
                                void* composed_layer);
void rsf_depth_replay_end_frame(rsf_depth_replay* replay);
#ifdef __cplusplus
}
#endif
#endif
