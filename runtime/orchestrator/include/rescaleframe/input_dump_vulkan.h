/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
typedef struct rsf_input_dump_vk rsf_input_dump_vk;
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
   Does no GPU work when the frame is not requested. No present semaphores are consumed. */
RSF_RUNTIME_API int32_t rsf_input_dump_vk_snapshot(rsf_input_dump_vk*, const rsf_frame_record*,
    const rsf_srvk_image*, const char* name);
/* Join previously submitted engine/capture commands before retiring their readbacks. */
RSF_RUNTIME_API int32_t rsf_input_dump_vk_drain(rsf_input_dump_vk*);
/* After engine work has drained. A failed fence join preserves the owner for a retry. */
RSF_RUNTIME_API int32_t rsf_input_dump_vk_destroy(rsf_input_dump_vk*);
#ifdef __cplusplus
}
#endif
