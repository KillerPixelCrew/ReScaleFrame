// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_sr_vulkan.h>

#define RSF_FGVK_ABI_VERSION 1u
typedef struct rsf_fgvk rsf_fgvk;
typedef struct rsf_fgvk_setup {
    uint32_t struct_size, abi_version;
    rsf_srvk_setup graphics;
    void* queues[4]; /* graphics, compute, present, acquire; distinct, same family */
    uint32_t queue_family;
    const void* create_info; /* borrowed VkSwapchainCreateInfoKHR for this call */
    const void* allocator; /* borrowed VkAllocationCallbacks, live through destroy */
    /* Host queue synchronization. SDK submits on the graphics queue through this
       callback; the host serializes it with its own submissions/presentation. */
    int32_t (*graphics_submit)(void* user, uint32_t count, const void* submits, uint64_t fence);
    void (*lock_queues)(void* user);
    void (*unlock_queues)(void* user);
    void* queue_user;
} rsf_fgvk_setup;

typedef struct rsf_fgvk_frame {
    uint32_t struct_size, abi_version;
    const rsf_frame_record* record;
    rsf_srvk_image depth, motion, hudless;
    float motion_scale_x, motion_scale_y;
} rsf_fgvk_frame;

typedef struct rsf_fgvk_status {
    uint32_t struct_size, abi_version;
    uint32_t enabled, available;
    int32_t result;
    uint64_t prepared, tagged, generated_callbacks, last_present_count, source_frame;
} rsf_fgvk_status;

#ifdef __cplusplus
extern "C" {
#endif
/* The service owns the entire replacement WSI table. Never send its chain to
   native Vulkan WSI. The graphics queue is externally synchronized by the engine;
   the other three queues are reserved exclusively for this owner. Serialize owner
   calls and retain queue callbacks through destroy. This version owns one primary
   window; secondary native windows retain their separate WSI ownership. */
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_create(const rsf_fgvk_setup*, rsf_fgvk**, uint64_t* chain);
RSF_RUNTIME_API int32_t rsf_fgvk_images(rsf_fgvk*, uint32_t* count, uint64_t* images);
RSF_RUNTIME_API int32_t rsf_fgvk_acquire(rsf_fgvk*, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index);
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_enable(rsf_fgvk*, uint32_t enabled);
/* Record prepare at the depth/motion producer, and HUD-less copying after
   tonemapping before canvas. Inputs are leased until actual GPU completion. */
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_prepare(rsf_fgvk*, void* commands, const rsf_fgvk_frame*);
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_hudless(rsf_fgvk*, void* commands, const rsf_fgvk_frame*);
/* Call only after a successful engine queue submission. This binds recorded
   source identity to submitted work; recording alone never enables FG. */
RSF_RUNTIME_API void rsf_fgvk_submitted(rsf_fgvk*, void* const* commands, uint32_t count);
RSF_RUNTIME_API int32_t rsf_fgvk_present(rsf_fgvk*, const void* present_info);
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_get_status(rsf_fgvk*, rsf_fgvk_status*);
/* Drain deferred SDK presentation and engine work before retiring engine views.
   A failing destroy preserves the owner, module and resources for a retry. */
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_drain(rsf_fgvk*);
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_destroy(rsf_fgvk*);
#ifdef __cplusplus
}
#endif
