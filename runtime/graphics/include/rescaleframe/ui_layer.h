/* SPDX-License-Identifier: GPL-3.0-only */
/* Two-slot D3D11 UI-layer ring at output resolution, with eight-bit RGBA coverage and transparent
   clears. Draw premultiplied colour without a depth view; composite as ui.rgb + (1-ui.a)*scene.
   Views/textures are layer-owned and borrowed by callers. The ring preserves the previous slot
   while the current one is drawn, but provides no fence: the caller must finish consumption before
   a slot is reused. Serialize creation, begin_frame, access, and destruction with render work. */

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

/* Two slots; external GPU consumers must finish before a slot cycles back. */
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
    /* Use a typeless texture with an sRGB RTV to encode linear shader output on write. The SRV
       remains plain UNORM, so a composite onto an encoded back buffer reads stored encoded colour
       without decoding it. Zero uses UNORM for both views. */
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

/* Mark current slot written; the owner uses the flag to skip an empty-layer composite. */
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
