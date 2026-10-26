// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/runtime.h>
#include <rescaleframe/sr_session.h>

#define RSF_SRVK_ABI_VERSION 1u
typedef struct rsf_srvk rsf_srvk;
typedef void (*rsf_vk_function)(void);
typedef rsf_vk_function (*rsf_vk_get_proc)(void* object, const char* name);

typedef struct rsf_srvk_setup {
    uint32_t struct_size, abi_version;
    void* instance;
    void* physical_device;
    void* device;
    rsf_vk_get_proc instance_proc, device_proc;
    uint32_t backend, quality, output_width, output_height;
    uint32_t depth_inverted, depth_infinite, motion_jittered;
    const char* runtime_directory_utf8;
} rsf_srvk_setup;

/* Borrowed level-zero image. Format/layout are Vulkan enum values. The device
   must match setup. No VkImage, view or allocation ownership crosses this ABI. */
typedef struct rsf_srvk_image {
    void* device;
    uint64_t image;
    uint32_t format, width, height, layout;
    /* Nonzero promises VK_IMAGE_USAGE_TRANSFER_SRC_BIT for opt-in readback. */
    uint32_t copy_source;
} rsf_srvk_image;

typedef struct rsf_srvk_frame {
    uint32_t struct_size, abi_version;
    const rsf_frame_record* record;
    rsf_srvk_image color, depth, motion, output, reactive, composition;
    float motion_scale_x, motion_scale_y, pre_exposure;
} rsf_srvk_frame;

typedef struct rsf_srvk_fallback {
    uint32_t struct_size, abi_version;
    void* instance;
    void* physical_device;
    void* device;
    rsf_vk_get_proc instance_proc, device_proc;
    void* command_buffer;
    rsf_srvk_image color, output;
} rsf_srvk_fallback;

#ifdef __cplusplus
extern "C" {
#endif
/* DLSS requires an early shared rsf_slvk_initialize session with DLSS SR
   selected. Its inputs must support transfer-source usage; depth is R32F.
   Only one DLSS SR owner is admitted per SDK session, on viewport one. */
RSF_RUNTIME_API rsf_backend_result rsf_srvk_create(const rsf_srvk_setup*, rsf_srvk**);
RSF_RUNTIME_API rsf_backend_result rsf_srvk_plan(const rsf_srvk*, uint32_t* width, uint32_t* height);
/* DLSS model request, applied on subsequent evaluations. The owner serializes
   calls and drains referencing GPU work before changing a live model. */
RSF_RUNTIME_API rsf_backend_result rsf_srvk_dlss_preset(rsf_srvk*, uint32_t preset);
/* Inputs: SHADER_READ_ONLY_OPTIMAL. Engine output: TRANSFER_DST_OPTIMAL. SDK
   output is private and reaches the engine only after an accepted dispatch.
   The owner serializes calls, leases images until GPU completion, and submits
   this command buffer even on failure, or retires the context instead of reuse.
   Drain all referencing GPU work before destroy. This API never submits a queue.
   RSF_VULKAN_FRAME_CAPTURE_DIR enables bounded readbacks, flushed at drained
   destruction. FIRST/COUNT use the RSF_FRAME_CAPTURE_* variables (at most 16).
   Capturing an input requires copy_source and the corresponding image usage. */
RSF_RUNTIME_API rsf_backend_result rsf_srvk_evaluate(rsf_srvk*, void* command_buffer, const rsf_srvk_frame*);
/* Spatial failure recovery, independent of an SDK context. Both images are
   single-sample RGBA16F level-zero images on device. Color requires copy_source
   and SHADER_READ_ONLY_OPTIMAL; output requires TRANSFER_DST usage/layout.
   Restores color layout and leaves output TRANSFER_DST_OPTIMAL. All validation
   precedes recording. The caller submits and leases both images through GPU
   completion, and retains the original SR failure in its provider status. */
RSF_RUNTIME_API rsf_backend_result rsf_srvk_spatial_fallback(const rsf_srvk_fallback*);
/* Failure preserves the context and its module/resources for a later retry. */
RSF_RUNTIME_API rsf_backend_result rsf_srvk_destroy(rsf_srvk*);
#ifdef __cplusplus
}
#endif
