// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_fg_vulkan.h>

#define RSF_DLSSGVK_ABI_VERSION 1u
typedef struct rsf_dlssgvk rsf_dlssgvk;
typedef struct rsf_dlssgvk_status {
    uint32_t struct_size, abi_version;
    rsf_fgvk_status presentation;
    uint32_t requested_generated_frames, max_generated_frames;
    uint32_t sdk_status, min_dimension, vsync_supported, dynamic_mfg_supported;
    int32_t sdk_result, asynchronous_present_result;
} rsf_dlssgvk_status;

#ifdef __cplusplus
extern "C" {
#endif
/* One primary window, viewport zero, in the already initialized shared
   Streamline session with RSF_SLVK_DLSS_FG. The setup/frame ABI is FGVK 1.
   WSI calls use the Streamline proxy chain and host's latency broker. Never
   send this chain to an FFX replacement owner or create a second SDK session.
   The host serializes calls and keeps its callbacks/allocator alive. */
RSF_RUNTIME_API rsf_backend_result rsf_dlssgvk_create(const rsf_fgvk_setup*, rsf_dlssgvk**, uint64_t* chain);
RSF_RUNTIME_API int32_t rsf_dlssgvk_images(rsf_dlssgvk*, uint32_t* count, uint64_t* images);
RSF_RUNTIME_API int32_t rsf_dlssgvk_acquire(rsf_dlssgvk*, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index);
/* Required final layout of each game-facing proxy image, including while FG
   is Off. This differs from FFX's shader-readable replacement images. */
RSF_RUNTIME_API rsf_backend_result rsf_dlssgvk_image_layout(const rsf_dlssgvk*, uint32_t* layout);
/* Generated count excludes the real frame: 1 means 2x, 3 means 4x. Unsupported
   counts are refused rather than clamped. Off preserves the requested count. */
RSF_RUNTIME_API rsf_backend_result rsf_dlssgvk_enable(rsf_dlssgvk*, uint32_t enabled, uint32_t generated_count);
RSF_RUNTIME_API rsf_backend_result rsf_dlssgvk_prepare(rsf_dlssgvk*, void* commands, const rsf_fgvk_frame*);
RSF_RUNTIME_API rsf_backend_result rsf_dlssgvk_hudless(rsf_dlssgvk*, void* commands, const rsf_fgvk_frame*);
/* Notify only after successful host submission. Each source requires both
   guide and HUD-less command buffers before presentation may enable FG. */
RSF_RUNTIME_API void rsf_dlssgvk_submitted(rsf_dlssgvk*, void* const* commands, uint32_t count);
RSF_RUNTIME_API int32_t rsf_dlssgvk_present(rsf_dlssgvk*, const void* present_info);
RSF_RUNTIME_API rsf_backend_result rsf_dlssgvk_get_status(rsf_dlssgvk*, rsf_dlssgvk_status*);
/* Drain before retiring engine images or changing providers. Failure preserves
   the owner, SDK feature, chain and resources for a later retry. */
RSF_RUNTIME_API rsf_backend_result rsf_dlssgvk_drain(rsf_dlssgvk*);
RSF_RUNTIME_API rsf_backend_result rsf_dlssgvk_destroy(rsf_dlssgvk*);
#ifdef __cplusplus
}
#endif
