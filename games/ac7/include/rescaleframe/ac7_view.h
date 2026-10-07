/* SPDX-License-Identifier: GPL-3.0-only */
/* Parse AC7's captured 4096-byte UE4.18 view-buffer allocation without GPU calls.
   Recognition checks size/reciprocal pairs, camera basis, projection inverse and camera-origin
   translation. Callers must also select the intended view; a valid auxiliary view is not the
   player camera. UE4.18 lacks ViewToClipNoAA, shifting later fields by 0x40 relative to newer UE.
   Layout evidence: docs/research/ac7-frame-capture.md and tools/verify-view-layout.py. */

#ifndef RSF_AC7_VIEW_H
#define RSF_AC7_VIEW_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_AC7_VIEW_ABI_VERSION 3u

/* Captured allocation size, not proof that the underlying engine structure occupies 4096 bytes. */
#define RSF_AC7_VIEW_BUFFER_BYTES 4096u

typedef int32_t rsf_ac7_view_result;
#define RSF_AC7_VIEW_OK ((rsf_ac7_view_result)0)
#define RSF_AC7_VIEW_ERROR_INVALID_ARGUMENT ((rsf_ac7_view_result)-1)
#define RSF_AC7_VIEW_ERROR_ABI_MISMATCH ((rsf_ac7_view_result)-2)
/* The bytes do not hold a view uniform buffer. Reported rather than parsed anyway. */
#define RSF_AC7_VIEW_ERROR_NOT_A_VIEW_BUFFER ((rsf_ac7_view_result)-3)
/* A view buffer, but an orthographic one: the interface renders through those. */
#define RSF_AC7_VIEW_ERROR_NOT_PERSPECTIVE ((rsf_ac7_view_result)-4)

typedef struct rsf_ac7_view {
    uint32_t struct_size;

    /* Row-major engine matrices. view_to_clip includes current projection jitter;
       view_to_clip_no_jitter removes its two clip-space offsets for backend camera input. */
    float view_to_clip[16];
    float view_to_clip_no_jitter[16];
    float clip_to_view[16];
    /* Inverse of the corrected projection; clip_to_view above retains engine jitter. */
    float clip_to_view_no_jitter[16];
    /* Copied engine transform from current clip coordinates to previous clip coordinates. */
    float clip_to_prev_clip[16];
    /* Inverted directly to avoid composing matrices with large world-space translations. */
    float prev_clip_to_clip[16];

    /* Position and near_plane use engine world units; basis vectors are dimensionless. */
    float camera_position[3];
    float camera_forward[3];
    float camera_up[3];
    float camera_right[3];

    /* Reversed Z with an infinite far plane, so there is a near value and no far value. */
    float near_plane;
    /* Vertical field of view in radians, derived from this view's projection. */
    float vertical_fov;
    float aspect_ratio;

    /* Active view rectangle and allocation dimensions in pixels; the allocation may be padded. */
    uint32_t view_width;
    uint32_t view_height;
    uint32_t view_rect_x;
    uint32_t view_rect_y;
    uint32_t buffer_width;
    uint32_t buffer_height;

    /* Geometric heuristic: rectangle starts at (0,0) and fills the allocation. */
    uint32_t is_main_view;

    /* Render-pixel offsets: clip x * width/2, clip y * -height/2. Uses the active view size.
       Values come from TemporalAAJitter.xy/zw, not caller-maintained history. Engine temporal
       preparation must be selected to produce jitter; see loader/README.md. */
    float jitter_pixels[2];
    float previous_jitter_pixels[2];
    /* Nonzero when either current clip-space jitter component is nonzero. */
    uint32_t has_jitter;
} rsf_ac7_view;

/* Parse borrowed, float-aligned buffer bytes into caller-owned out. Set out->struct_size first;
   bytes must equal RSF_AC7_VIEW_BUFFER_BYTES and abi_version must match this header. Returns the
   specific argument/ABI/layout/perspective error. Treat out as unusable after any error because
   a later matrix/perspective rejection may leave partial output. No input pointer is retained. */
rsf_ac7_view_result rsf_ac7_view_read(const void* buffer, uint32_t bytes, uint32_t abi_version,
                                      rsf_ac7_view* out);

/* Copy bytes from in to a separate, non-overlapping out buffer and remove current projection
   jitter for UI/translucency rendered outside temporal reconstruction. Both buffers must be
   float-aligned and cover at least 0x800 bytes. Invalid arguments leave out untouched; a valid
   copy returns 0 for zero jitter or unsupported perspective form, and 1 after correction.
   This helper does not perform the reader's complete view-buffer recognition.

   UE4.18 adds TemporalAAJitter.xy to ViewToClip[2][0/1] and derives all current matrices from it
   (SceneView.h:402, SceneView.cpp:2259). For column 3 = (0,0,1,0), ViewToClip' = ViewToClip * J,
   where J has (jx,jy) in row 3. Corrections are:
     into clip (TranslatedWorldToClip, WorldToClip, ViewToClip): column 0 -= jx * column 3,
       column 1 -= jy * column 3;
     out of clip (ClipToView, ClipToTranslatedWorld): row 3 += jx * row 0 + jy * row 1;
     screen to world (ScreenToWorld, ScreenToTranslatedWorld): row 2 += jx * row 0 + jy * row 1;
     SVPositionToTranslatedWorld: row 3 += jx * ClipToTranslatedWorld row 0 + jy * row 1;
     TemporalAAJitter.xy = 0.
   Previous matrices and TemporalAAJitter.zw retain the history used by engine velocity. */
uint32_t rsf_ac7_view_remove_jitter(const void* in, void* out, uint32_t bytes);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_AC7_VIEW_H */
