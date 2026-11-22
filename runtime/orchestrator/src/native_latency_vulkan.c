// SPDX-License-Identifier: MIT
#include <stddef.h>
#include <rescaleframe/native_cpu_service.h>
#include "native_latency_vulkan_platform.h"

#if defined(RSF_HAVE_STREAMLINE_VK)
void* VulkanStorySlModule(void);
int VulkanStorySlShutdownChecked(void);
rsf_vk_function VulkanStorySlGetInstanceProcAddr(void*, const char*);
int VulkanStorySlBindReflex(void);
int VulkanStorySlBindPcl(void);
uint32_t VulkanStorySlPclWindowMessage(void);
int VulkanStorySlGetReflexState(uint32_t*, uint32_t*);
int VulkanStorySlSetReflexInterval(int, uint32_t);
int rsf_vk_sl_begin_frame_checked(uint32_t, uintptr_t*);
uintptr_t VulkanStorySlCurrentFrameToken(void);
int VulkanStorySlTokenMatches(uintptr_t, uint32_t);
int VulkanStorySlReflexSleep(void);
int VulkanStorySlMarkerForToken(uintptr_t, uint32_t);
int VulkanStorySlQueuePresentSynchronized(uintptr_t, void*, const void*);
int VulkanStorySlVulkanWsiCall(rsf_vk_wsi_call, void*);
int VulkanStorySlWithoutDlssG(rsf_vk_wsi_call, void*);
int32_t rsf_sllatency_initialize_checked(const uint16_t*, const char*, uint32_t, uint32_t, uint32_t, int32_t*);
int32_t rsf_sllatency_invoke_checked(const rsf_slvk_call*, void*, uintptr_t);

static const rsf_vk_streamline_calls streamline = {
    rsf_sllatency_initialize_checked, VulkanStorySlShutdownChecked, VulkanStorySlModule, rsf_sllatency_invoke_checked,
    VulkanStorySlSetReflexInterval, VulkanStorySlReflexSleep,
    {rsf_vk_sl_begin_frame_checked, VulkanStorySlCurrentFrameToken, VulkanStorySlTokenMatches,
        VulkanStorySlMarkerForToken},
    {VulkanStorySlBindPcl, VulkanStorySlPclWindowMessage, VulkanStorySlBindReflex,
        VulkanStorySlGetReflexState},
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
    return rsf_core_slvk_without_dlssg(VulkanStorySlWithoutDlssG, callback, user);
#else
    return rsf_core_slvk_without_dlssg(NULL, callback, user);
#endif
}

rsf_vk_function rsf_latencyvk_instance_proc(void* instance, const char* name) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    // Resolver remains usable after slShutdown for Vulkan object destruction.
    return rsf_core_latencyvk_instance_proc(VulkanStorySlGetInstanceProcAddr, instance, name);
#else
    return rsf_core_latencyvk_instance_proc(NULL, instance, name);
#endif
}

int32_t rsf_latencyvk_queue_present(rsf_vk_function callback, void* queue, const void* info) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    return rsf_core_latencyvk_queue_present(VulkanStorySlQueuePresentSynchronized, callback, queue, info);
#else
    return rsf_core_latencyvk_queue_present(NULL, callback, queue, info);
#endif
}

int32_t rsf_latencyvk_wsi_call(rsf_vk_wsi_call callback, void* user) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    return rsf_core_latencyvk_wsi_call(VulkanStorySlVulkanWsiCall, callback, user);
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
