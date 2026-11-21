// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_fg_vulkan.h>
#include "vulkan_fidelityfx_native.h"

struct rsf_ffxgvk;
extern "C" {
rsf_backend_result rsf_ffxgvk_create(const rsf_fgvk_setup*, rsf_ffxgvk**, uint64_t* chain);
int32_t rsf_ffxgvk_images(rsf_ffxgvk*, uint32_t* count, uint64_t* images);
int32_t rsf_ffxgvk_acquire(rsf_ffxgvk*, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index);
rsf_backend_result rsf_ffxgvk_enable(rsf_ffxgvk*, uint32_t enabled);
rsf_backend_result rsf_ffxgvk_prepare(rsf_ffxgvk*, void* commands, const rsf_fgvk_frame*);
rsf_backend_result rsf_ffxgvk_hudless(rsf_ffxgvk*, void* commands, const rsf_fgvk_frame*);
void rsf_ffxgvk_submitted(rsf_ffxgvk*, void* const* commands, uint32_t count);
int32_t rsf_ffxgvk_present(rsf_ffxgvk*, const void* present_info);
int32_t rsf_ffxgvk_present_source(rsf_ffxgvk*, const void* present_info, const rsf_frame_record*);
rsf_backend_result rsf_ffxgvk_get_status(rsf_ffxgvk*, rsf_fgvk_status*);
rsf_backend_result rsf_ffxgvk_drain(rsf_ffxgvk*);
rsf_backend_result rsf_ffxgvk_destroy(rsf_ffxgvk*);
}

/* Private native leaf for the Rust owner in vulkan_ffx_fg.rs. The leaf keeps
   the FidelityFX module, SDK contexts, the WSI hook bodies and their
   marshalling, command recording, capture and timing. Rust decides setup and
   swapchain admission, the process-wide WSI route, slot reuse, copy transitions,
   completion evidence, presentation admission and retirement order. Status-returning
   steps report rsf_backend_result; WSI steps return the VkResult unchanged.
   Creation-only steps read the borrowed create_info and run only inside
   rsf_ffxgvk_create. Rust admits the route before open and publishes the leaf
   after successful WSI registration. It closes the route to callbacks and
   unregisters the WSI table before closing the leaf and releasing admission. */
struct rsf_ffxgvk_native;
struct rsf_vkfg_platform;
struct rsf_ffxgvk_swapchain {
    uint32_t width, height, format, color_space, layers, structure_valid;
    uint64_t old_chain;
};
struct rsf_ffxgvk_barrier {
    uint64_t image;
    uint32_t old_layout, new_layout, source_family, target_family;
    uint32_t source_access, target_access, source_stages, target_stages;
};
struct rsf_ffxgvk_image_copy {
    uint64_t source, target;
    uint32_t source_layout, target_layout, width, height;
};
struct rsf_ffxgvk_ops {
    rsf_vk_sdk_loader loader;
    int32_t (*open)(const rsf_fgvk_setup*, void* module, uint32_t width, uint32_t height, rsf_ffxgvk_native**, void** resources);
    void (*close)(rsf_ffxgvk_native*);
    /* Returns the corrected SDK's dispatch registration result. */
    int32_t (*set_wsi)(rsf_ffxgvk_native*, uint32_t enabled);
    /* Ownership facts are valid on refusal as well as success. */
    int32_t (*create_swapchain)(rsf_ffxgvk_native*, uint64_t* chain, uint32_t* has_context);
    int32_t (*query_swapchain)(rsf_ffxgvk_native*);
    int32_t (*create_effect)(rsf_ffxgvk_native*, uint32_t* has_context);
    int32_t (*query_provider)(rsf_ffxgvk_native*);
    int32_t (*destroy_effect)(rsf_ffxgvk_native*);
    void (*destroy_chain)(rsf_ffxgvk_native*);
    int32_t (*destroy_swapchain)(rsf_ffxgvk_native*);
    int32_t (*images)(rsf_ffxgvk_native*, uint32_t*, uint64_t*);
    int32_t (*acquire)(rsf_ffxgvk_native*, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t*, uint64_t frame);
    int32_t (*prepare)(rsf_ffxgvk_native*, void* commands, const rsf_fgvk_frame*);
    void (*barrier)(rsf_ffxgvk_native*, void* commands, const rsf_ffxgvk_barrier*);
    void (*copy)(rsf_ffxgvk_native*, void* commands, const rsf_ffxgvk_image_copy*);
    int32_t (*configure)(rsf_ffxgvk_native*, uint32_t enabled, uint64_t color, uint64_t frame, const char* stage);
    int32_t (*present)(rsf_ffxgvk_native*, const void* info, uint64_t frame, uint64_t* present_count);
    int32_t (*drain)(rsf_ffxgvk_native*);
    uint32_t (*capture_wants)(rsf_ffxgvk_native*, uint64_t frame);
    void (*capture_record)(rsf_ffxgvk_native*, void* commands, const rsf_srvk_image*, const rsf_frame_record*, const char* role);
    void (*capture_flush)(rsf_ffxgvk_native*);
    void (*fence_timing)(rsf_ffxgvk_native*, uint64_t frame);
    int64_t (*timing_begin)();
    void (*timing_end)(const char* stage, uint64_t frame, int64_t start);
    const rsf_vkfg_platform* resources;
};

extern "C" {
/* A null ops table reports a disabled build after validating the arguments. */
rsf_backend_result rsf_core_ffxgvk_create(const rsf_ffxgvk_ops*, const rsf_fgvk_setup*, const rsf_ffxgvk_swapchain*,
    rsf_ffxgvk**, uint64_t* chain);
int32_t rsf_core_ffxgvk_images(rsf_ffxgvk*, uint32_t*, uint64_t*);
int32_t rsf_core_ffxgvk_acquire(rsf_ffxgvk*, uint64_t, uint64_t, uint64_t, uint32_t*);
rsf_backend_result rsf_core_ffxgvk_enable(rsf_ffxgvk*, uint32_t);
rsf_backend_result rsf_core_ffxgvk_prepare(rsf_ffxgvk*, void*, const rsf_fgvk_frame*);
rsf_backend_result rsf_core_ffxgvk_hudless(rsf_ffxgvk*, void*, const rsf_fgvk_frame*);
void rsf_core_ffxgvk_submitted(rsf_ffxgvk*, void* const*, uint32_t);
int32_t rsf_core_ffxgvk_present(rsf_ffxgvk*, const void* info, uint64_t chain, const rsf_frame_record*, uint32_t explicit_source);
rsf_backend_result rsf_core_ffxgvk_status(rsf_ffxgvk*, rsf_fgvk_status*);
rsf_backend_result rsf_core_ffxgvk_drain(rsf_ffxgvk*);
rsf_backend_result rsf_core_ffxgvk_destroy(rsf_ffxgvk*);
/* Scoped WSI borrow. Returns the published leaf, or null before publication,
   once retirement has closed the route, or, with match_device, for another
   device. Pair each non-null result with one rsf_core_ffxgvk_wsi_return. */
rsf_ffxgvk_native* rsf_core_ffxgvk_wsi_borrow(const void* device, uint32_t match_device);
void rsf_core_ffxgvk_wsi_return();
/* Invoked by the SDK generation callback within Present. Copies the source
   record for this leaf and frame, then counts dispatch only when it succeeds. */
uint32_t rsf_core_ffxgvk_generation_record(const rsf_ffxgvk_native*, uint64_t frame, rsf_frame_record*);
void rsf_core_ffxgvk_note_generated(const rsf_ffxgvk_native*);
}
