// SPDX-License-Identifier: MIT
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_fg_vulkan.h>

extern "C" {
struct rsf_vk_token_calls {
    uintptr_t (*begin)(uint32_t);
    uintptr_t (*current)();
    int32_t (*matches)(uintptr_t, uint32_t);
    int32_t (*mark)(uintptr_t, uint32_t);
};
struct rsf_vk_pcl_calls {
    int32_t (*bind)();
    uint32_t (*message)();
    void (*reflex)(uint32_t*, uint32_t*);
    void (*report)(int32_t);
};
struct rsf_vk_streamline_calls {
    int32_t (*initialize)(const wchar_t*, const char*, uint32_t, int32_t*);
    int32_t (*shutdown)();
    void* (*module)();
    int32_t (*invoke)(const rsf_slvk_call*, void*, uintptr_t);
    void (*pace)(uint64_t, uint32_t, uint32_t, uint32_t, int32_t*);
    rsf_vk_token_calls tokens;
    rsf_vk_pcl_calls pcl;
};
struct rsf_vk_latency_platform {
    const rsf_vk_streamline_calls* streamline;
    int (*dispatch)(const rsf_game_cpu_event*, rsf_game_cpu_event_fn, void*);
    rsf_backend_result (*backend)(const rsf_fgvk*, uint32_t*);
    rsf_backend_result (*options)(rsf_fgvk*, uint32_t, uint32_t);
    rsf_backend_result (*sleep)(rsf_fgvk*, uint64_t);
    rsf_backend_result (*marker)(rsf_fgvk*, uint64_t, uint32_t);
    rsf_backend_result (*status)(rsf_fgvk*, rsf_fgvk_latency_status*);
};

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
void rsf_core_latencyvk_sdk_result(int32_t, uint32_t marker);
}

#if defined(RSF_HAVE_STREAMLINE_VK)
#include "native_vulkan_timing.h"

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
int VulkanStorySlBeginFrame(uint32_t);
uintptr_t VulkanStorySlCurrentFrameToken();
int VulkanStorySlTokenMatches(uintptr_t, uint32_t);
int VulkanStorySlReflexSleep();
int VulkanStorySlMarkerForToken(uintptr_t, uint32_t);
int VulkanStorySlQueuePresentSynchronized(uintptr_t, void*, const void*);
int VulkanStorySlVulkanWsiCall(rsf_vk_wsi_call, void*);
int VulkanStorySlWithoutDlssG(rsf_vk_wsi_call, void*);
}

namespace {
int32_t initialize(const wchar_t* directory, const char* project, uint32_t features, int32_t* result) noexcept try {
    *result = VulkanStorySlInitializeFeatures(directory, project, 1,
        (features & RSF_SLVK_DLSS_FG) != 0, (features & RSF_SLVK_DLSS_SR) != 0);
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

int32_t invoke(const rsf_slvk_call* call, void* module, uintptr_t token) noexcept try {
    return call->callback(module, token, call->user);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

void pace(uint64_t frame, uint32_t mode, uint32_t limit, uint32_t sleep, int32_t* results) {
    const rsf::VulkanTimingScope timing("cpu_pacing_total", frame);
    results[0] = rsf::vk_timed_call("reflex_options",frame,[&] {
        return VulkanStorySlSetReflexInterval(static_cast<int>(mode),limit);
    });
    results[1] = results[0]==0 && sleep ? rsf::vk_timed_call("reflex_sleep",frame,VulkanStorySlReflexSleep) : 0;
}

constexpr rsf_vk_streamline_calls streamline{
    initialize, VulkanStorySlShutdownChecked, VulkanStorySlModule, invoke, pace,
    {
        [](uint32_t index) -> uintptr_t {
            // The Rust host holds its guard for both calls. The bridge owns the
            // current pointer, and the host ledger associates it with a source.
            const auto result = VulkanStorySlBeginFrame(index);
            rsf_core_latencyvk_sdk_result(result, 0);
            return result==0 ? VulkanStorySlCurrentFrameToken() : 0;
        },
        VulkanStorySlCurrentFrameToken,
        VulkanStorySlTokenMatches,
        [](uintptr_t token, uint32_t kind) {
            const auto result = VulkanStorySlMarkerForToken(token, kind);
            rsf_core_latencyvk_sdk_result(result, 1);
            return result;
        },
    },
    {
        VulkanStorySlBindPcl,
        VulkanStorySlPclWindowMessage,
        [](uint32_t* available, uint32_t* reports) {
            if (VulkanStorySlBindReflex()==0) (void)VulkanStorySlGetReflexState(available,reports);
        },
        [](int32_t result) { rsf_core_latencyvk_sdk_result(result, 0); },
    },
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
    rsf_native_cpu_dispatch, rsf_fgvk_backend, rsf_fgvk_latency_options, rsf_fgvk_latency_sleep,
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
