// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_sr_vulkan.h>

/* FSR4 uses the existing Vulkan/DX12 interop owner and a graph recording split.
   This contract is independent of native SR ABI 1, which remains unchanged. */
#define RSF_SRVK_SPLIT_ABI_VERSION 1u
typedef struct rsf_fsr4vk rsf_fsr4vk;
typedef struct rsf_fsr4vk_handoff {
    uint32_t struct_size, abi_version;
    /* Caller enabled timeline semaphores. Both buffers use this actual queue
       family, not EXTERNAL, FOREIGN or IGNORED. */
    uint32_t timeline_enabled, queue_family;
    void* user;
    /* End current_commands and return a DIFFERENT recording command buffer.
       At normal graph execution the producer waits previous_done (if nonzero)
       and signals ready; the continuation waits done. Use ALL_COMMANDS wait
       stages. Both sections and successive evaluations must be submitted in
       order on the SAME host queue. Submit a frame before evaluating the next;
       do not collect multiple unsubmitted frames. timeline is a borrowed
       permanently imported VkSemaphore.
       Return zero on success, nonzero on failure. Do not submit here or throw
       across the C ABI. Failure makes the owner
       terminal: neither buffer may be reused, and destroy retains the owner. */
    int32_t (*split_record)(void* user, void* current_commands, uint64_t timeline,
        uint64_t previous_done, uint64_t ready, uint64_t done, void** continuation_commands);
} rsf_fsr4vk_handoff;

#ifdef __cplusplus
extern "C" {
#endif
/* Serialize all calls on the graphics owner. Sizes accept appended fields;
   versions must match exactly. Validation uses RSF_BACKEND_ERROR_*; other native
   Vulkan, HRESULT and FidelityFX errors pass through as opaque nonzero int32_t
   results. Missing implementation returns NOT_COMPILED, never another SR.
   setup.backend must be RSF_SR_FSR4. runtime_directory_utf8 locates the signed
   DX12 provider. This API adds no vendor restriction to the existing provider.
   Devices, callback code and user data outlive checked destruction. Strings are
   borrowed only during create. Failed cleanup can retain *owner on failure:
   keep it for destroy; plan/evaluate are invalid until create succeeds. */
RSF_RUNTIME_API int32_t rsf_srvk_fsr4_create(const rsf_srvk_setup*, const rsf_fsr4vk_handoff*, rsf_fsr4vk** owner);
RSF_RUNTIME_API int32_t rsf_srvk_fsr4_plan(const rsf_fsr4vk*, uint32_t* width, uint32_t* height);
/* One stable view/session per owner, strictly increasing frame IDs. Drain and
   recreate for a new size, quality or view. Images are single-sample, level-zero,
   whole 2D images: RGBA16F color/output, R32F depth, RG16F motion. Inputs require
   TRANSFER_SRC usage, copy_source=1 and SHADER_READ_ONLY_OPTIMAL; output requires
   TRANSFER_DST usage/layout. Images belong to setup.device and queue_family.
   Reactive/composition masks are unsupported and must be empty. Lease images
   through GPU completion of BOTH graph sections. Validation failure returns
   current_commands without recording. A successful split returns the new buffer
   INCLUDING SDK failure: record spatial fallback there, submit both sections and
   preserve the provider error. Split failure returns NULL: retire the graph/owner
   instead of reusing either buffer. Success leaves inputs shader-read and output
   transfer-dst. Never abandon an owner with unsubmitted graph sections. */
RSF_RUNTIME_API int32_t rsf_srvk_fsr4_evaluate(rsf_fsr4vk*, void* current_commands,
    const rsf_srvk_frame*, void** continuation_commands);
RSF_RUNTIME_API void rsf_srvk_fsr4_retire_commands(rsf_fsr4vk*, void* commands);
/* Submit both sections and drain their GPU users first. Success frees the owner;
   failure retains it and remaining resource/module leases. NULL is a successful
   no-op. These entry points never submit the host's Vulkan queue. */
RSF_RUNTIME_API int32_t rsf_srvk_fsr4_destroy(rsf_fsr4vk*);
#ifdef __cplusplus
}
#endif
