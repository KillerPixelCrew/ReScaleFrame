/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_AC7_SCENE_COLOR_H
#define RSF_AC7_SCENE_COLOR_H

#include <rescaleframe/frame_tap.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Zero-initialize. Render-thread owned; the two textures are retained until replacement or clear.
   Recognition is a capture-derived draw shape, not a verified shader identity. */
typedef struct rsf_ac7_scene_color {
    void* source;
    void* composed;
    void* context;
    uint32_t width;
    uint32_t height;
    uint32_t composed_this_frame;
    uint32_t search_closed;
} rsf_ac7_scene_color;

/* Returns nonzero when the input watch must be changed to state->source. */
int rsf_ac7_scene_color_source(rsf_ac7_scene_color* state, void* source, void* context,
                             uint32_t width, uint32_t height);
void rsf_ac7_scene_color_draw(rsf_ac7_scene_color* state, const rsf_frame_tap_target_draw* draw);
/* Borrowed result; falls back unless this frame observed a matching draw for this source. */
void* rsf_ac7_scene_color_selected(const rsf_ac7_scene_color* state, void* source);
void rsf_ac7_scene_color_end_frame(rsf_ac7_scene_color* state);
void rsf_ac7_scene_color_clear(rsf_ac7_scene_color* state);

#ifdef __cplusplus
}
#endif
#endif
