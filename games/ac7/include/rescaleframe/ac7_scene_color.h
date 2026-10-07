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
    /* Retained candidate RGBA16F layer identities for the current recombine. Stale D3D11 slots
       can also satisfy the descriptor, so consumers must compare their exact replayed layer
       against this bounded set. Evidence: docs/research/ac7-composed-scene-color.md. */
    void* composed_layers[8];
    uint32_t composed_layer_count;
    void* context; /* Borrowed immediate-context identity; never retained or dereferenced here. */
    uint32_t width;
    uint32_t height;
    uint32_t composed_this_frame;
    uint32_t search_closed;
} rsf_ac7_scene_color;

/* Replace retained source and invalidate composition when identity, context or size changes.
   Returns nonzero when the input watch must be changed to state->source. Arguments are borrowed. */
int rsf_ac7_scene_color_source(rsf_ac7_scene_color* state, void* source, void* context,
                             uint32_t width, uint32_t height);
/* Observe one borrowed draw; retains resources only after the captured recombine shape matches. */
void rsf_ac7_scene_color_draw(rsf_ac7_scene_color* state, const rsf_frame_tap_target_draw* draw);
/* Borrowed result; falls back unless this frame observed a matching draw for this source. */
void* rsf_ac7_scene_color_selected(const rsf_ac7_scene_color* state, void* source);
/* Release current-frame layer candidates and reopen discovery; source/composed stay retained. */
void rsf_ac7_scene_color_end_frame(rsf_ac7_scene_color* state);
/* Captured separate-layer draw shape. Depth/raster suitability is checked by graphics replay. */
int rsf_ac7_scene_depth_candidate(const rsf_frame_tap_geometry* draw);
/* The same capture-derived separate-translucency shape, available before its first draw. */
int rsf_ac7_separate_translucency_draw(const rsf_frame_tap_target_draw* draw);
/* Target and render percentages are relative to output, not percentages of each other.
   Zero target retains the diagnostic match-scene mode. Zero render means native. */
float rsf_ac7_translucency_scale(uint32_t render_percent, uint32_t target_percent);
/* Release every retained texture and return state to its zero-initialized form. state is required. */
void rsf_ac7_scene_color_clear(rsf_ac7_scene_color* state);

#ifdef __cplusplus
}
#endif
#endif
