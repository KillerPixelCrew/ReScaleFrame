/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_DEPTH_SCALE_ABI_VERSION 1u
#define RSF_DEPTH_SCALE_OK 0
#define RSF_DEPTH_SCALE_INVALID_ARGUMENT (-1)
#define RSF_DEPTH_SCALE_ABI_MISMATCH (-2)
#define RSF_DEPTH_SCALE_RESOURCE_FAILED (-3)
#define RSF_DEPTH_SCALE_BUSY (-4)

typedef struct rsf_depth_scale rsf_depth_scale;
typedef struct rsf_depth_scale_setup {
    uint32_t struct_size, abi_version;
    void (*log)(void*, const char*);
    void* log_user;
} rsf_depth_scale_setup;
typedef struct rsf_depth_scale_frame {
    uint32_t struct_size, abi_version;
    /* ID3D11Texture2D*, R24G8_TYPELESS, single mip/slice/sample, DEFAULT depth target.
       Retained from successful prepare through restore/cancel. Contents must remain
       unchanged between prepare and commit. All operations use one immediate context. */
    void* texture;
    /* Pixel bounds [left, top, right, bottom], within the allocation. */
    int32_t source_rect[4], output_rect[4];
    /* Source pixels to subtract from the mapped destination sample position.
       Positive x is right; positive y is down. Nearest texel, clamped to source_rect. */
    float jitter_pixels[2];
} rsf_depth_scale_frame;

/* One graphics-thread transaction. Create retains the immediate context and device.
   Prepare snapshots the complete allocation and renders a separate image, preserving
   all eight stencil bits and every pixel outside output_rect. It never writes texture.
   Commit copies that image into texture. Restore queues the exact original contents
   back, or cancels an uncommitted preparation, and releases the texture lease.
   The owner must restore after the final consumer and before the next native writer.
   No GPU wait is needed on the same immediate context. Destroy restores if committed.
   Wrong-thread calls return BUSY without touching graphics or destroying the handle.
   These are graphics primitives; callers own frame identity and consumer lifetime. */
int32_t rsf_depth_scale_create(void* immediate_context, const rsf_depth_scale_setup*, rsf_depth_scale**);
int32_t rsf_depth_scale_prepare(rsf_depth_scale*, const rsf_depth_scale_frame*);
int32_t rsf_depth_scale_commit(rsf_depth_scale*);
int32_t rsf_depth_scale_restore(rsf_depth_scale*);
/* Release the native texture lease without restoring its contents, preserving
   writes made by later native consumers. Graphics owner only. */
int32_t rsf_depth_scale_discard(rsf_depth_scale*);
int32_t rsf_depth_scale_destroy(rsf_depth_scale*);

#ifdef __cplusplus
}
#endif
