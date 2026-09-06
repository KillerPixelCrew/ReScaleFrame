/* SPDX-License-Identifier: GPL-3.0-only */
/* Read Ace Combat 7's view uniform buffer.

   The camera data a reconstruction backend cannot do without is not in a frame capture and not
   derivable from one. It lives in a 4096 byte constant buffer the engine binds every frame, and
   this turns those bytes into the values a backend asks for.

   Reading it is only half the job. At runtime nothing labels a constant buffer, so whatever finds
   one has to be able to say whether it really is the view buffer, and which view it describes. The
   engine renders several per frame, and three of eleven captured here describe viewports of
   1016x1016 and 128x93 inside the same 2048x1152 target. Handing a backend the camera of a view
   the player is not looking through would produce a plausible, wrong image.

   So `rsf_ac7_view_read` refuses rather than guesses. It checks relationships that hold in a view
   buffer and essentially nowhere else: sizes paired with their reciprocals, a camera basis that is
   orthonormal and matches the rows of ViewToTranslatedWorld, a projection and its inverse that
   multiply to the identity, and a translation that is the negated camera position. A buffer that
   is not one fails several of them at once.

   The layout is stock Unreal 4.18 with a single difference: `ViewToClipNoAA` does not exist in
   4.18, so every field after `ViewToClip` sits 0x40 earlier than a later engine puts it. That one
   shift is why reading the stock layout stops working partway through. The offsets, and the
   identity that establishes them, are in docs/research/ac7-frame-capture.md and re-checked against
   captured buffers by tools/verify-view-layout.py. */

#ifndef RSF_AC7_VIEW_H
#define RSF_AC7_VIEW_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_AC7_VIEW_ABI_VERSION 2u

/* The engine binds the view uniform data at exactly this size. A buffer of any other size is not
   one, which is the cheapest test available and the first one applied. */
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

    /* Row major, the convention both Unreal and Streamline use, so these are copies rather than
       transposes.

       `view_to_clip` carries the projection jitter when it is enabled, because 4.18 applies the
       offset to the projection itself and keeps no un-jittered copy: `ViewToClipNoAA` arrived in a
       later engine version. Backends require the matrix without it, so `view_to_clip_no_jitter` is
       the same matrix with the two elements the engine wrote taken back out. */
    float view_to_clip[16];
    float view_to_clip_no_jitter[16];
    float clip_to_view[16];
    /* Read from the buffer rather than composed: the engine computes exactly the matrix a backend
       asks for. */
    float clip_to_prev_clip[16];
    /* Inverted here, because the engine keeps no such field. `clip_to_prev_clip` is close to the
       identity, so inverting it is well conditioned, which composing world space matrices with
       coordinates in the hundreds of thousands is not. */
    float prev_clip_to_clip[16];

    float camera_position[3];
    float camera_forward[3];
    float camera_up[3];
    float camera_right[3];

    /* Reversed Z with an infinite far plane, so there is a near value and no far value. */
    float near_plane;
    /* Radians. Per frame rather than constant: 38.0, 58.7 and 33.4 degrees all appear. */
    float vertical_fov;
    float aspect_ratio;

    uint32_t view_width;
    uint32_t view_height;
    uint32_t view_rect_x;
    uint32_t view_rect_y;
    uint32_t buffer_width;
    uint32_t buffer_height;

    /* Whether this is the view the player is looking through, rather than one of the smaller ones
       the engine renders into the same target. */
    uint32_t is_main_view;

    /* Sub-pixel projection offset, in pixels at the view's own resolution, which is the convention
       every backend takes it in. The engine stores it in clip space and divides by the view rect
       rather than by the buffer, so this is that construction run backwards.

       Zero unless the anti-aliasing gate has been patched: 4.18 computes a jitter only for a view
       asking for temporal AA, and Ace Combat 7 never asks. See loader/README.md.

       The previous frame's offset sits beside the current one in the buffer, and a backend that
       wants it does not have to remember the last frame. */
    float jitter_pixels[2];
    float previous_jitter_pixels[2];
    /* Whether the projection carries a jitter at all this frame. A backend handed an unjittered
       projection can sharpen but cannot recover detail, so this is worth reporting rather than
       leaving a caller to compare two floats against zero. */
    uint32_t has_jitter;
} rsf_ac7_view;

/* Read `bytes` of constant buffer into `out`, or refuse.

   `abi_version` is passed rather than stored in the struct because the caller is the one who has
   to be compiled against a matching header. */
rsf_ac7_view_result rsf_ac7_view_read(const void* buffer, uint32_t bytes, uint32_t abi_version,
                                      rsf_ac7_view* out);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_AC7_VIEW_H */
