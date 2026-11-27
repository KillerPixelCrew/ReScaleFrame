/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
typedef struct rsf_input_dump_vk rsf_input_dump_vk;
#define RSF_INPUT_DUMP_VK_DEVICE_LOST (-13)
typedef struct rsf_input_dump_vk_setup {
    uint32_t struct_size, abi_version;
    rsf_srvk_setup graphics;
    void* queue;
    uint32_t queue_family;
    /* The engine serializes this submission with its graphics queue. */
    int32_t (*submit)(void* user, uint32_t count, const void* submits, uint64_t fence);
    void* user;
} rsf_input_dump_vk_setup;
#ifdef __cplusplus
extern "C" {
#endif
RSF_RUNTIME_API int32_t rsf_input_dump_vk_create(const rsf_input_dump_vk_setup*, rsf_input_dump_vk**);
/* Capture-only synchronous snapshot. The source's rendering must already be submitted on
   setup.queue. Records barriers/copy, restores layout, and waits a bounded completion fence.
   Does no GPU work when the frame is not requested. No present semaphores are consumed.
   A timed-out submission retains use of the source until its fence retires. */
RSF_RUNTIME_API int32_t rsf_input_dump_vk_snapshot(rsf_input_dump_vk*, const rsf_frame_record*,
    const rsf_srvk_image*, const char* name);
/* Join the capture submission and drain the supplied queue. Device loss is terminal. */
RSF_RUNTIME_API int32_t rsf_input_dump_vk_drain(rsf_input_dump_vk*);
/* After engine work has drained. Serialize destruction with calls using this handle.
   A pending fence preserves the owner for a retry. A fence reporting device loss
   permits teardown without publication. */
RSF_RUNTIME_API int32_t rsf_input_dump_vk_destroy(rsf_input_dump_vk*);
#ifdef __cplusplus
}
#endif
