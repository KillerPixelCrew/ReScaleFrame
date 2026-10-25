// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
#include <rescaleframe/game_renderer.h>

#define RSF_LATENCYVK_ABI_VERSION 1u
typedef struct rsf_latencyvk_setup {
    uint32_t struct_size, abi_version;
    const char* runtime_directory_utf8;
    const char* project_id;
} rsf_latencyvk_setup;
typedef struct rsf_latencyvk_status {
    uint32_t struct_size, abi_version;
    uint32_t low_latency_available, report_available, pcl_message, mode;
    uint64_t sleep_calls, marker_calls, refused_calls;
    int32_t last_result;
} rsf_latencyvk_status;

#ifdef __cplusplus
extern "C" {
#endif
/* One serialized Streamline Vulkan session per process. Initialize before
   Vulkan, then use this instance resolver for every Vulkan dispatch table.
   The interposer stays mapped after shutdown until process exit. */
RSF_RUNTIME_API rsf_backend_result rsf_latencyvk_initialize(const rsf_latencyvk_setup*);
RSF_RUNTIME_API rsf_vk_function rsf_latencyvk_instance_proc(void* instance, const char* name);
/* Call after device creation, on the simulation/input thread. The SDK PCL
   options bind its ping message to this thread. Unsupported nonzero modes fail. */
RSF_RUNTIME_API rsf_backend_result rsf_latencyvk_options(uint32_t mode, uint32_t frame_limit_us);
RSF_RUNTIME_API void rsf_latencyvk_cpu(const rsf_game_cpu_event*);
RSF_RUNTIME_API rsf_backend_result rsf_latencyvk_input(uint64_t frame, uint32_t kind, uint32_t message);
RSF_RUNTIME_API void rsf_latencyvk_render(uint64_t frame);
/* Called around the real root-window Present, including camera-free frames. */
RSF_RUNTIME_API void rsf_latencyvk_present(uint32_t begin);
/* Serialize the immediate WSI call with LL2 markers/sleep on the SDK's lock.
   The callback and borrowed queue/info remain valid through this call. */
RSF_RUNTIME_API int32_t rsf_latencyvk_queue_present(rsf_vk_function callback, void* queue, const void* info);
typedef int32_t (*rsf_vk_wsi_call)(void*);
/* Synchronous native WSI callback lease, also used for acquire/image queries.
   Do not reenter PCL/Reflex from the callback. No user data is retained. */
RSF_RUNTIME_API int32_t rsf_latencyvk_wsi_call(rsf_vk_wsi_call, void* user);
RSF_RUNTIME_API rsf_backend_result rsf_latencyvk_get_status(rsf_latencyvk_status*);
/* Caller first retires provider work. Dispatch code remains mapped for device
   and instance destruction after this call. */
RSF_RUNTIME_API rsf_backend_result rsf_latencyvk_shutdown(void);
#ifdef __cplusplus
}
#endif
