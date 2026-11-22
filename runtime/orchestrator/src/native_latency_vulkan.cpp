// SPDX-License-Identifier: MIT
#include <rescaleframe/native_cpu_service.h>
#include "native_latency_vulkan_platform.h"

#if defined(RSF_HAVE_STREAMLINE_VK)
extern "C" {
int VulkanStorySlInitializeFeatures(const wchar_t*, const char*, uint32_t, uint32_t, uint32_t);
void* VulkanStorySlModule();
int VulkanStorySlShutdownChecked();
rsf_vk_function VulkanStorySlGetInstanceProcAddr(void*, const char*);
int VulkanStorySlBindReflex();
int VulkanStorySlBindPcl();
uint32_t VulkanStorySlPclWindowMessage();
int VulkanStorySlGetReflexState(uint32_t*, uint32_t*);
int VulkanStorySlSetReflexInterval(int, uint32_t);
int rsf_vk_sl_begin_frame_checked(uint32_t, uintptr_t*);
uintptr_t VulkanStorySlCurrentFrameToken();
int VulkanStorySlTokenMatches(uintptr_t, uint32_t);
int VulkanStorySlReflexSleep();
int VulkanStorySlMarkerForToken(uintptr_t, uint32_t);
int VulkanStorySlQueuePresentSynchronized(uintptr_t, void*, const void*);
int VulkanStorySlVulkanWsiCall(rsf_vk_wsi_call, void*);
int VulkanStorySlWithoutDlssG(rsf_vk_wsi_call, void*);
}

namespace {
// C++ owns only optional provider linkage and the exception boundary.
int32_t initialize(const uint16_t* directory, const char* project, uint32_t reflex,
    uint32_t fg, uint32_t sr, int32_t* result) noexcept try {
    *result = VulkanStorySlInitializeFeatures(reinterpret_cast<const wchar_t*>(directory),
        project, reflex, fg, sr);
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

int32_t invoke(const rsf_slvk_call* call, void* module, uintptr_t token) noexcept try {
    return call->callback(module, token, call->user);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

constexpr rsf_vk_streamline_calls streamline{
    initialize, VulkanStorySlShutdownChecked, VulkanStorySlModule, invoke,
    VulkanStorySlSetReflexInterval, VulkanStorySlReflexSleep,
    {rsf_vk_sl_begin_frame_checked, VulkanStorySlCurrentFrameToken, VulkanStorySlTokenMatches,
        VulkanStorySlMarkerForToken},
    {VulkanStorySlBindPcl, VulkanStorySlPclWindowMessage, VulkanStorySlBindReflex,
        VulkanStorySlGetReflexState},
};
}
#endif

namespace {
constexpr rsf_vk_latency_platform platform{
#if defined(RSF_HAVE_STREAMLINE_VK)
    &streamline,
#else
    nullptr,
#endif
    rsf_fgvk_backend, rsf_fgvk_latency_options, rsf_fgvk_latency_sleep,
    rsf_fgvk_latency_marker, rsf_fgvk_latency_get_status,
};
}

extern "C" rsf_backend_result rsf_latencyvk_initialize(const rsf_latencyvk_setup* setup) {
    return rsf_core_latencyvk_initialize(&platform, setup);
}
extern "C" rsf_backend_result rsf_slvk_initialize(const rsf_slvk_setup* setup) {
    return rsf_core_slvk_initialize(&platform, setup);
}
extern "C" rsf_backend_result rsf_slvk_invoke(const rsf_slvk_call* call) {
    return rsf_core_slvk_invoke(&platform, call);
}
extern "C" rsf_backend_result rsf_latencyvk_options(uint32_t mode, uint32_t limit) {
    return rsf_core_latencyvk_options(&platform, mode, limit);
}
extern "C" rsf_backend_result rsf_latencyvk_presentation(void* context) {
    return rsf_core_latencyvk_presentation(&platform, static_cast<rsf_fgvk*>(context));
}
extern "C" void rsf_latencyvk_submit_end(uint64_t frame) {
    rsf_core_latencyvk_submit_end(&platform, frame);
}
extern "C" int rsf_latencyvk_ping(void* window) {
    return rsf_core_latencyvk_pcl_ping(window);
}
extern "C" void rsf_latencyvk_cpu(const rsf_game_cpu_event* event) {
    rsf_core_latencyvk_cpu(&platform, event);
}
extern "C" rsf_backend_result rsf_latencyvk_input(uint64_t frame, uint32_t kind, uint32_t message) {
    return rsf_core_latencyvk_input(&platform, 0, frame, kind, message);
}
extern "C" void rsf_latencyvk_render(uint64_t frame) {
    rsf_core_latencyvk_render(&platform, frame);
}
extern "C" void rsf_latencyvk_present(uint32_t begin) {
    rsf_core_latencyvk_present(&platform, begin);
}
extern "C" rsf_backend_result rsf_latencyvk_execution(const rsf_game_render_pass* pass, uint32_t begin) {
    return rsf_core_latencyvk_submit(&platform, pass, begin);
}
extern "C" rsf_backend_result rsf_latencyvk_get_status(rsf_latencyvk_status* status) {
    return rsf_core_latencyvk_get_status(&platform, status);
}
extern "C" rsf_backend_result rsf_latencyvk_shutdown() {
    return rsf_core_latencyvk_shutdown(&platform);
}

extern "C" int32_t rsf_slvk_without_dlssg(int32_t (*callback)(void*), void* user) {
    if (!callback) return -3;
#if defined(RSF_HAVE_STREAMLINE_VK)
    return VulkanStorySlWithoutDlssG(callback, user);
#else
    return callback(user);
#endif
}
extern "C" rsf_vk_function rsf_latencyvk_instance_proc(void* instance, const char* name) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    // Resolver remains usable after slShutdown for Vulkan object destruction.
    return name ? VulkanStorySlGetInstanceProcAddr(instance, name) : nullptr;
#else
    (void)instance; (void)name; return nullptr;
#endif
}
extern "C" int32_t rsf_latencyvk_queue_present(rsf_vk_function callback, void* queue, const void* info) {
    if (!callback || !queue || !info) return -3;
#if defined(RSF_HAVE_STREAMLINE_VK)
    return VulkanStorySlQueuePresentSynchronized(reinterpret_cast<uintptr_t>(callback), queue, info);
#else
    using Present = int32_t (*)(void*, const void*);
    return reinterpret_cast<Present>(callback)(queue, info);
#endif
}
extern "C" int32_t rsf_latencyvk_wsi_call(rsf_vk_wsi_call callback, void* user) {
    if (!callback || !user) return -3;
#if defined(RSF_HAVE_STREAMLINE_VK)
    return VulkanStorySlVulkanWsiCall(callback, user);
#else
    return callback(user);
#endif
}

extern "C" const rsf_cpu_session_service* rsf_latencyvk_cpu_service()
{
    static constexpr rsf_cpu_session_service service{sizeof(service), RSF_CPU_SESSION_SERVICE_VERSION,
        rsf_latencyvk_cpu, rsf_latencyvk_ping, rsf_latencyvk_execution};
    return &service;
}
