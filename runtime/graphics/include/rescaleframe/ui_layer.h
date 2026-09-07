/* SPDX-License-Identifier: GPL-3.0-only */
/* The surface the game's interface is drawn into instead of into the scene.

   This is the thing that makes the interface sharp. AC7 rasterizes its front end at a fixed
   1920x1080 and then draws it as world space quads into a render-resolution layer of its own, which
   the game upscales along with everything else. Promotion cannot fix that: there is no target whose
   promotion sharpens geometry that was rasterized into the scene. Moving those draws into a layer
   at output resolution can, and this is the layer.

   What it has to be is decided by the vendors rather than by us, because the same surface is handed
   to frame generation later and all three SDKs ask for the same thing:

     - `R8G8B8A8_UNORM`, never the back buffer's `R10G10B10A2`. Two bits of alpha cannot express
       partial coverage, and coverage is the whole content of this surface.
     - Premultiplied: colour already scaled by its own alpha, so the composite is an add.
     - Alpha zero where there is no interface, which means cleared to zero and not to black.
     - The back buffer's extent, because that is what it is composited against.

   Never bound with a depth stencil view. The draws being moved here bind the scene's depth, and
   keeping that would mean depth testing the interface against a scene it is no longer part of, as
   well as requiring the two extents to match. Depth decides what a divert does, not what the layer
   is.

   Double buffered. A frame generation vendor may hold the layer until the next present rather than
   consuming it during the call, so the frame being drawn cannot be the frame being read. */

#ifndef RSF_UI_LAYER_H
#define RSF_UI_LAYER_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_UI_LAYER_ABI_VERSION 1u

typedef int32_t rsf_ui_layer_result;
#define RSF_UI_LAYER_OK ((rsf_ui_layer_result)0)
#define RSF_UI_LAYER_ERROR_INVALID_ARGUMENT ((rsf_ui_layer_result)-1)
#define RSF_UI_LAYER_ERROR_ABI_MISMATCH ((rsf_ui_layer_result)-2)
#define RSF_UI_LAYER_ERROR_RESOURCE_FAILED ((rsf_ui_layer_result)-3)

/* How many frames the ring holds. Two is enough for a vendor that reads the previous frame's layer
   while this one is being drawn, and more would only delay the memory being reused. */
#define RSF_UI_LAYER_RING 2u

typedef void (*rsf_ui_layer_log_fn)(void* user, const char* message);

typedef struct rsf_ui_layer_setup {
    uint32_t struct_size;
    uint32_t abi_version;
    /* The extent to composite against, which is the back buffer's. */
    uint32_t width;
    uint32_t height;
    /* Also create the layer shareable, for handing to a D3D12 frame generation vendor across the
       presentation bridge. Costs nothing when unused and cannot be added later without recreating,
       which is why it is asked for at creation. */
    uint32_t shareable;
    /* Write through an sRGB view, so a shader's linear output is encoded on the way in exactly as
       it was in the target the draws were taken from.
     *
     * This is not cosmetic. Unreal allocates its targets typeless and chooses per view; a draw that
       was being encoded and now is not stores linear values where encoded ones are expected, and
       the whole layer comes out dark and desaturated while every count says it worked. The texture
       is created typeless so both views can exist over it, and the composite reads back through a
       matching sRGB view so the decode undoes the encode exactly. */
    uint32_t srgb;
    rsf_ui_layer_log_fn log;
    void* log_user;
} rsf_ui_layer_setup;

typedef struct rsf_ui_layer rsf_ui_layer;

rsf_ui_layer_result rsf_ui_layer_create(void* d3d11_device, const rsf_ui_layer_setup* setup,
                                        rsf_ui_layer** out);
void rsf_ui_layer_destroy(rsf_ui_layer* layer);

/* Begin a frame: clear the slot about to be drawn into, and make it the current one.

   The clear is here rather than after the composite because a layer is only known to be finished
   when the next one starts. A frame that presents twice, or not at all, then costs a stale layer
   rather than a black screen. */
rsf_ui_layer_result rsf_ui_layer_begin_frame(rsf_ui_layer* layer, void* context);

/* The current slot's render target view, for a diverted draw to write into. Null before the first
   `begin_frame`. Borrowed: the layer keeps it alive and the caller must not release it. */
void* rsf_ui_layer_target(rsf_ui_layer* layer);

/* The current slot's shader resource view, for the composite to read. */
void* rsf_ui_layer_source(rsf_ui_layer* layer);

/* The current slot's `ID3D11Texture2D*`, for a caller that needs the resource itself, such as one
   opening it on another device. */
void* rsf_ui_layer_texture(rsf_ui_layer* layer);

/* Record that something was drawn into the layer this frame.

   The composite is skipped when nothing was, which matters more than it sounds: a compositing draw
   over every frame that has no interface on it is pure cost, and a run that reports zero drawn and
   still composites is describing a bug. */
void rsf_ui_layer_mark_written(rsf_ui_layer* layer);

typedef struct rsf_ui_layer_status {
    uint32_t struct_size;
    uint32_t width;
    uint32_t height;
    uint32_t slot;
    uint32_t shareable;
    /* Non-zero when a draw reached the current slot since `begin_frame`. */
    uint32_t written_this_frame;
    uint64_t frames_begun;
    uint64_t frames_written;
    uint64_t clears;
} rsf_ui_layer_status;

rsf_ui_layer_result rsf_ui_layer_get_status(const rsf_ui_layer* layer,
                                            rsf_ui_layer_status* status);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_UI_LAYER_H */
