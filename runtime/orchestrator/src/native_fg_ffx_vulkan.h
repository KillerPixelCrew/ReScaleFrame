/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/native_fg_vulkan.h>

typedef struct rsf_ffxgvk rsf_ffxgvk;

/* Host services the Rust FidelityFX 3 leaf (vulkan_ffx_fg/sdk.rs) borrows for
   every owner created with this table, so the table outlives those owners.
   load/unload/symbol open the SDK module. resolve and memory_properties call a
   host Vulkan resolver or vkGetPhysicalDeviceMemoryProperties and contain a
   foreign C++ exception as RSF_BACKEND_ERROR_INIT_FAILED. The three latency
   entries have the rsf_slvk_without_dlssg, rsf_latencyvk_wsi_call and
   rsf_latencyvk_queue_present contracts. */
typedef int32_t (*rsf_ffxgvk_scope)(int32_t (*callback)(void*), void* user);
typedef struct rsf_ffxgvk_host {
    void* (*load)(const uint16_t* path);
    void (*unload)(void* module);
    rsf_vk_function (*symbol)(void* module, const char* name);
    int32_t (*resolve)(rsf_vk_get_proc resolve, void* object, const char* name, rsf_vk_function* out);
    int32_t (*memory_properties)(rsf_vk_function query, void* physical_device, void* properties);
    rsf_ffxgvk_scope without_dlssg;
    rsf_ffxgvk_scope wsi_call;
    int32_t (*queue_present)(rsf_vk_function present, void* queue, const void* info);
} rsf_ffxgvk_host;

#ifdef __cplusplus
extern "C" {
#endif
/* Provider entries (vulkan_ffx_fg.rs), which vulkan_fg.rs dispatches to. */
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

/* Exception containment for host Vulkan calls (vulkan_ffx_fg/sdk.rs), in the production host table. */
int32_t rsf_ffxgvk_guard_resolve(rsf_vk_get_proc resolve, void* object, const char* name, rsf_vk_function* out);
int32_t rsf_ffxgvk_guard_memory(rsf_vk_function query, void* physical_device, void* properties);

/* Rust owner. Rust admits the setup and the process-wide WSI route, then
   opens the SDK through the host table. A null host reports a disabled build
   after validating the arguments. WSI steps return the VkResult unchanged. */
rsf_backend_result rsf_core_ffxgvk_create_sdk(const rsf_ffxgvk_host*, const rsf_fgvk_setup*,
    rsf_ffxgvk**, uint64_t* chain);
/* Scoped WSI borrow of the published SDK leaf. Returns null before
   publication, once retirement has closed the route, or, with match_device,
   for another device. Pair each non-null result with one return call. */
void* rsf_core_ffxgvk_wsi_borrow(const void* device, uint32_t match_device);
void rsf_core_ffxgvk_wsi_return(void);

/* Rust SDK module services shared with the Vulkan SR leaf. */
void* rsf_core_vk_sdk_load(const uint16_t* path);
void rsf_core_vk_sdk_unload(void* module);
rsf_vk_function rsf_vkffx_symbol(void* module, const char* name);
#ifdef __cplusplus
}
#endif
