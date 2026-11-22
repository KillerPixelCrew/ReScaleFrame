// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include <rescaleframe/native_fg_vulkan.h>

#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_vk_token_calls {
    int32_t (*begin)(uint32_t, uintptr_t*);
    uintptr_t (*current)(void);
    int32_t (*matches)(uintptr_t, uint32_t);
    int32_t (*mark)(uintptr_t, uint32_t);
} rsf_vk_token_calls;
typedef struct rsf_vk_pcl_calls {
    int32_t (*bind)(void);
    uint32_t (*message)(void);
    int32_t (*bind_reflex)(void);
    int32_t (*reflex)(uint32_t*, uint32_t*);
} rsf_vk_pcl_calls;
typedef struct rsf_vk_streamline_calls {
    int32_t (*initialize)(const uint16_t*, const char*, uint32_t, uint32_t, uint32_t, int32_t*);
    int32_t (*shutdown)(void);
    void* (*module)(void);
    int32_t (*invoke)(const rsf_slvk_call*, void*, uintptr_t);
    int32_t (*set_interval)(int32_t, uint32_t);
    int32_t (*sleep)(void);
    rsf_vk_token_calls tokens;
    rsf_vk_pcl_calls pcl;
} rsf_vk_streamline_calls;
typedef struct rsf_vk_latency_platform {
    const rsf_vk_streamline_calls* streamline;
    rsf_backend_result (*backend)(const rsf_fgvk*, uint32_t*);
    rsf_backend_result (*options)(rsf_fgvk*, uint32_t, uint32_t);
    rsf_backend_result (*sleep)(rsf_fgvk*, uint64_t);
    rsf_backend_result (*marker)(rsf_fgvk*, uint64_t, uint32_t);
    rsf_backend_result (*status)(rsf_fgvk*, rsf_fgvk_latency_status*);
} rsf_vk_latency_platform;

rsf_backend_result rsf_core_latencyvk_initialize(const rsf_vk_latency_platform*, const rsf_latencyvk_setup*);
rsf_backend_result rsf_core_slvk_initialize(const rsf_vk_latency_platform*, const rsf_slvk_setup*);
rsf_backend_result rsf_core_slvk_invoke(const rsf_vk_latency_platform*, const rsf_slvk_call*);
rsf_backend_result rsf_core_latencyvk_presentation(const rsf_vk_latency_platform*, rsf_fgvk*);
rsf_backend_result rsf_core_latencyvk_options(const rsf_vk_latency_platform*, uint32_t, uint32_t);
void rsf_core_latencyvk_cpu(const rsf_vk_latency_platform*, const rsf_game_cpu_event*);
int32_t rsf_core_latencyvk_pcl_ping(void*);
rsf_backend_result rsf_core_latencyvk_input(const rsf_vk_latency_platform*, uint64_t, uint64_t, uint32_t, uint32_t);
void rsf_core_latencyvk_submit_end(const rsf_vk_latency_platform*, uint64_t);
void rsf_core_latencyvk_render(const rsf_vk_latency_platform*, uint64_t);
void rsf_core_latencyvk_present(const rsf_vk_latency_platform*, uint32_t);
rsf_backend_result rsf_core_latencyvk_submit(const rsf_vk_latency_platform*, const rsf_game_render_pass*, uint32_t);
rsf_backend_result rsf_core_latencyvk_get_status(const rsf_vk_latency_platform*, rsf_latencyvk_status*);
rsf_backend_result rsf_core_latencyvk_shutdown(const rsf_vk_latency_platform*);
typedef int32_t (*rsf_vk_wsi_scope)(rsf_vk_wsi_call, void*);
typedef int32_t (*rsf_vk_synchronized_present)(uintptr_t, void*, const void*);
int32_t rsf_core_slvk_without_dlssg(rsf_vk_wsi_scope, rsf_vk_wsi_call, void*);
rsf_vk_function rsf_core_latencyvk_instance_proc(rsf_vk_get_proc, void*, const char*);
int32_t rsf_core_latencyvk_queue_present(rsf_vk_synchronized_present, rsf_vk_function, void*, const void*);
int32_t rsf_core_latencyvk_wsi_call(rsf_vk_wsi_scope, rsf_vk_wsi_call, void*);
#ifdef __cplusplus
}
#endif
