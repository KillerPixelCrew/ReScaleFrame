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

/* Why the last draw was not replayed, or zero if the last one was.

   A replay that draws nothing looks identical to one that was never asked, and five separate tests
   can reject a draw. This says which:

     1  nothing usable in the report
     2  a different size than this replay was built for
     3  this draw's shape: primitive kind, topology, no vertex shader, no elements
     4  the depth view's format, dimension or read-only flag
     5  the depth view's resource is not a 2D texture
     6  that texture's size, format or sample count
     7  the pipeline has a stage or binding this cannot reproduce
     8  the depth stencil state is not the translucency one
     9  the copy or the draw itself failed

   Reasons 4 and above stop the replay for the rest of the frame, because they describe the frame
   rather than the draw. Reasons 2 and 3 skip one draw and let the next be judged on its own. */
uint32_t rsf_depth_replay_last_reject(const rsf_depth_replay* replay);

/* What the last candidate actually looked like, so a refusal can be read instead of guessed at.
   Zeroed fields mean that stage was never reached. */
typedef struct rsf_depth_replay_detail {
    uint32_t draw_width, draw_height, draw_samples;
    uint32_t dsv_format, dsv_dimension, dsv_flags;
    uint32_t source_width, source_height, source_format, source_samples;
    /* What the selection predicate compares. It hands over its texture only when the layer, the
       depth and the context all match what the caller is about to submit, and a mismatch in any one
       of them looks identical from outside: replayed draws, nothing selected. */
    void* layer;
    void* source;
    void* context;
    uint32_t draws;
    uint32_t refused;
} rsf_depth_replay_detail;

void rsf_depth_replay_get_detail(const rsf_depth_replay* replay, rsf_depth_replay_detail* out);
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
