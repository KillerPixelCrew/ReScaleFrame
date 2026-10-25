// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_sr_vulkan.h>

#define RSF_FGVK_ABI_VERSION 1u
#define RSF_FGVK_HOST_ABI_VERSION 1u
#define RSF_FGVK_LATENCY_ABI_VERSION 1u
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

typedef struct rsf_fgvk_host_options {
    uint32_t struct_size, abi_version;
    void* window; /* Borrowed HWND; live through owner destruction. */
    uint32_t timeline_enabled;
    const uint16_t* intel_runtime_directory; /* Absolute UTF-16 path, borrowed during create. */
} rsf_fgvk_host_options;

typedef struct rsf_fgvk_latency_status {
    uint32_t struct_size, abi_version;
    uint32_t mode, available;
    uint64_t sleep_calls, marker_calls;
    int32_t result; /* Original XeLL result; counts cover successful SDK calls. */
} rsf_fgvk_latency_status;

#ifdef __cplusplus
extern "C" {
#endif
/* The service owns the entire provider WSI table. Never send its chain to a
   different WSI owner. Serialize calls and retain setup callbacks through destroy.
   There is one process-wide primary owner across providers; secondary windows
   retain native WSI ownership. FFX needs four distinct queues in one family.
   DLSS needs the early shared session with DLSS FG selected. */
/* Legacy creation selects FFX3 with one generated frame. */
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_create(const rsf_fgvk_setup*, rsf_fgvk**, uint64_t* chain);
/* Explicit FG selection is independent of setup.graphics.backend (SR). Backend
   1 selects DLSS; 3 selects FFX3; 4 selects FSR4 FG; 5 selects XeSS. FSR4 and
   XeSS require host options. FSR4 accepts count one and uses the graphics setup's
   FidelityFX runtime directory; it never falls back to FFX3 or an Intel directory.
   Count excludes the real frame. Unsupported
   providers/counts are refused. In the exceptional case of failed cleanup after
   partial creation, nonnull owner/chain outputs retain ownership for destroy
   retry even though this call fails. Never discard those retained outputs. */
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_create_for_backend(const rsf_fgvk_setup*, uint32_t backend,
    uint32_t generated_count, rsf_fgvk**, uint64_t* chain);
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_create_with_host(const rsf_fgvk_setup*, uint32_t backend,
    uint32_t generated_count, const rsf_fgvk_host_options*, rsf_fgvk**, uint64_t* chain);
RSF_RUNTIME_API int32_t rsf_fgvk_images(rsf_fgvk*, uint32_t* count, uint64_t* images);
RSF_RUNTIME_API int32_t rsf_fgvk_acquire(rsf_fgvk*, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index);
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_enable(rsf_fgvk*, uint32_t enabled);
/* Legacy enable preserves the selected count. Options update only on success. */
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_enable_options(rsf_fgvk*, uint32_t enabled, uint32_t generated_count);
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_backend(const rsf_fgvk*, uint32_t* backend);
/* Final layout of the game-facing images for this owner's entire lifetime,
   including Off: FFX/FSR4/XeSS use shader-read, DLSS uses transfer-source. Vulkan enum value. */
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_image_layout(const rsf_fgvk*, uint32_t* layout);
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_max_generated(rsf_fgvk*, uint32_t* count);
/* Separate XeLL control for the XeSS presentation owner. Mode is Off(0)/On(1);
   Boost is unsupported. Frame IDs must fit the SDK's nonzero uint32_t domain. */
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_latency_options(rsf_fgvk*, uint32_t mode, uint32_t minimum_interval_us);
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_latency_sleep(rsf_fgvk*, uint64_t frame);
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_latency_marker(rsf_fgvk*, uint64_t frame, uint32_t marker);
RSF_RUNTIME_API rsf_backend_result rsf_fgvk_latency_get_status(rsf_fgvk*, rsf_fgvk_latency_status*);
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
