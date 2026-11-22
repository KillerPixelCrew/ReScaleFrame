// SPDX-License-Identifier: MIT
#include <stddef.h>
#include <rescaleframe/native_cpu_service.h>
#include "native_latency_vulkan_platform.h"

#if defined(RSF_HAVE_STREAMLINE_VK)
int32_t rsf_vk_sl_initialize_features(const wchar_t*, const char*, uint32_t, uint32_t, uint32_t);
void* rsf_vk_sl_module(void);
int rsf_vk_sl_shutdown_checked(void);
rsf_vk_function rsf_vk_sl_get_instance_proc_addr(void*, const char*);
int rsf_vk_sl_bind_reflex(void);
int rsf_vk_sl_bind_pcl(void);
uint32_t rsf_vk_sl_pcl_window_message(void);
int rsf_vk_sl_get_reflex_state(uint32_t*, uint32_t*);
int rsf_vk_sl_set_reflex_interval(int, uint32_t);
int rsf_vk_sl_begin_frame_checked(uint32_t, uintptr_t*);
uintptr_t rsf_vk_sl_current_frame_token(void);
int rsf_vk_sl_token_matches(uintptr_t, uint32_t);
int rsf_vk_sl_reflex_sleep(void);
int rsf_vk_sl_marker_for_token(uintptr_t, uint32_t);
int rsf_vk_sl_queue_present_synchronized(uintptr_t, void*, const void*);
int rsf_vk_sl_vulkan_wsi_call(rsf_vk_wsi_call, void*);
int rsf_vk_sl_without_dlss_g(rsf_vk_wsi_call, void*);
static const rsf_vk_streamline_calls streamline = {
    rsf_vk_sl_initialize_features, rsf_vk_sl_shutdown_checked, rsf_vk_sl_module, rsf_sllatency_invoke_checked,
    rsf_vk_sl_set_reflex_interval, rsf_vk_sl_reflex_sleep,
    {rsf_vk_sl_begin_frame_checked, rsf_vk_sl_current_frame_token, rsf_vk_sl_token_matches,
        rsf_vk_sl_marker_for_token},
    {rsf_vk_sl_bind_pcl, rsf_vk_sl_pcl_window_message, rsf_vk_sl_bind_reflex,
        rsf_vk_sl_get_reflex_state},
};
#endif

static const rsf_vk_latency_platform platform = {
#if defined(RSF_HAVE_STREAMLINE_VK)
    &streamline,
#else
    NULL,
#endif
    rsf_fgvk_backend, rsf_fgvk_latency_options, rsf_fgvk_latency_sleep,
    rsf_fgvk_latency_marker, rsf_fgvk_latency_get_status,
};

rsf_backend_result rsf_latencyvk_initialize(const rsf_latencyvk_setup* setup) {
    return rsf_core_latencyvk_initialize(&platform, setup);
}
rsf_backend_result rsf_slvk_initialize(const rsf_slvk_setup* setup) {
    return rsf_core_slvk_initialize(&platform, setup);
}
rsf_backend_result rsf_slvk_invoke(const rsf_slvk_call* call) {
    return rsf_core_slvk_invoke(&platform, call);
}
rsf_backend_result rsf_latencyvk_options(uint32_t mode, uint32_t limit) {
    return rsf_core_latencyvk_options(&platform, mode, limit);
}
rsf_backend_result rsf_latencyvk_presentation(void* context) {
    return rsf_core_latencyvk_presentation(&platform, context);
}
void rsf_latencyvk_submit_end(uint64_t frame) {
    rsf_core_latencyvk_submit_end(&platform, frame);
}
int rsf_latencyvk_ping(void* window) {
    return rsf_core_latencyvk_pcl_ping(window);
}
void rsf_latencyvk_cpu(const rsf_game_cpu_event* event) {
    rsf_core_latencyvk_cpu(&platform, event);
}
rsf_backend_result rsf_latencyvk_input(uint64_t frame, uint32_t kind, uint32_t message) {
    return rsf_core_latencyvk_input(&platform, 0, frame, kind, message);
}
void rsf_latencyvk_render(uint64_t frame) {
    rsf_core_latencyvk_render(&platform, frame);
}
void rsf_latencyvk_present(uint32_t begin) {
    rsf_core_latencyvk_present(&platform, begin);
}
rsf_backend_result rsf_latencyvk_execution(const rsf_game_render_pass* pass, uint32_t begin) {
    return rsf_core_latencyvk_submit(&platform, pass, begin);
}
rsf_backend_result rsf_latencyvk_get_status(rsf_latencyvk_status* status) {
    return rsf_core_latencyvk_get_status(&platform, status);
}
rsf_backend_result rsf_latencyvk_shutdown(void) {
    return rsf_core_latencyvk_shutdown(&platform);
}

int32_t rsf_slvk_without_dlssg(rsf_vk_wsi_call callback, void* user) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    return rsf_core_slvk_without_dlssg(rsf_vk_sl_without_dlss_g, callback, user);
#else
    return rsf_core_slvk_without_dlssg(NULL, callback, user);
#endif
}

rsf_vk_function rsf_latencyvk_instance_proc(void* instance, const char* name) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    // Resolver remains usable after slShutdown for Vulkan object destruction.
    return rsf_core_latencyvk_instance_proc(rsf_vk_sl_get_instance_proc_addr, instance, name);
#else
    return rsf_core_latencyvk_instance_proc(NULL, instance, name);
#endif
}

int32_t rsf_latencyvk_queue_present(rsf_vk_function callback, void* queue, const void* info) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    return rsf_core_latencyvk_queue_present(rsf_vk_sl_queue_present_synchronized, callback, queue, info);
#else
    return rsf_core_latencyvk_queue_present(NULL, callback, queue, info);
#endif
}

int32_t rsf_latencyvk_wsi_call(rsf_vk_wsi_call callback, void* user) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    return rsf_core_latencyvk_wsi_call(rsf_vk_sl_vulkan_wsi_call, callback, user);
#else
    return rsf_core_latencyvk_wsi_call(NULL, callback, user);
#endif
}

const rsf_cpu_session_service* rsf_latencyvk_cpu_service(void)
{
    static const rsf_cpu_session_service service = {sizeof(service), RSF_CPU_SESSION_SERVICE_VERSION,
        rsf_latencyvk_cpu, rsf_latencyvk_ping, rsf_latencyvk_execution};
    return &service;
}
