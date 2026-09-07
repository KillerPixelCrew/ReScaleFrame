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
    /* Retained identities of the layers this frame's recombine reads.

       Every half-float input of the right size is a candidate, and there is usually more than one:
       D3D11 leaves slots bound, so a recombine arrives with whatever the previous draws left
       alongside what it actually reads. This used to keep one and null it the moment a second
       appeared, which was safe while separate translucency was half resolution and only one input
       could match. Rendering it at full resolution made two of them match and the layer became null
       every frame, so nothing was ever handed over.

       Keeping all of them removes the guess rather than making a better one. The caller has the
       texture its own replay drew into, and asking whether that is among these is the exact
       question; picking one here would only be inventing an answer to it. */
    void* composed_layers[8];
    uint32_t composed_layer_count;
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
/* Captured separate-layer draw shape. Depth/raster suitability is checked by graphics replay. */
int rsf_ac7_scene_depth_candidate(const rsf_frame_tap_geometry* draw);
void rsf_ac7_scene_color_clear(rsf_ac7_scene_color* state);

#ifdef __cplusplus
}
#endif
#endif
