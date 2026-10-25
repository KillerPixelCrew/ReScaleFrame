// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include <array>
#include <mutex>
#include <string>
#include <windows.h>

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
int VulkanStorySlBeginFrame(uint32_t);
uintptr_t VulkanStorySlCurrentFrameToken();
int VulkanStorySlTokenMatches(uintptr_t, uint32_t);
int VulkanStorySlReflexSleep();
int VulkanStorySlMarkerForToken(uintptr_t, uint32_t);
int VulkanStorySlQueuePresentSynchronized(uintptr_t, void*, const void*);
int VulkanStorySlVulkanWsiCall(rsf_vk_wsi_call, void*);
}
#endif
namespace {
std::mutex mutex;
bool initialized = false, bound = false;
uint32_t mode = 0, interval = 0;
uint64_t render_frame = 0, present_frame = 0, session = 0;
struct Token { uint64_t frame = 0; uintptr_t token = 0; uint32_t stages = 0; };
// The pinned SDK retains six handles. Validate its current index before every
// use, since a new acquisition can replace an older SDK-owned handle.
std::array<Token, 6> tokens{};
rsf_latencyvk_status status{sizeof(status), RSF_LATENCYVK_ABI_VERSION};
#if defined(RSF_HAVE_STREAMLINE_VK)
bool accepted(int result) {
    status.last_result = result;
    if (result) ++status.refused_calls;
    return result == 0;
}
void marker(uint64_t frame, uint32_t kind) {
    auto& slot = tokens[frame % tokens.size()];
    if (!bound || !frame || slot.frame != frame || !VulkanStorySlTokenMatches(slot.token, static_cast<uint32_t>(frame)) || (slot.stages & (1u << kind))) return;
    if (accepted(VulkanStorySlMarkerForToken(slot.token, kind))) {
        slot.stages |= 1u << kind; ++status.marker_calls;
    }
}
bool bind() {
    if (bound) return true;
    if (!accepted(VulkanStorySlBindPcl()) || !accepted(VulkanStorySlBindReflex())) return false;
    status.pcl_message = VulkanStorySlPclWindowMessage();
    if (!accepted(VulkanStorySlGetReflexState(&status.low_latency_available, &status.report_available))) return false;
    bound = true;
    return true;
}
#endif
}
extern "C" rsf_backend_result rsf_latencyvk_initialize(const rsf_latencyvk_setup* setup) try {
    if (!setup || setup->struct_size < sizeof(*setup)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_LATENCYVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    const rsf_slvk_setup shared{sizeof(shared), RSF_SLVK_ABI_VERSION, setup->runtime_directory_utf8, setup->project_id, 0};
    return rsf_slvk_initialize(&shared);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" rsf_backend_result rsf_slvk_initialize(const rsf_slvk_setup* setup) try {
    if (!setup || setup->struct_size < sizeof(*setup)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_SLVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (setup->features & ~RSF_SLVK_DLSS_SR) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!setup->runtime_directory_utf8 || !setup->project_id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard lock(mutex);
    if (initialized) return RSF_BACKEND_ERROR_NOT_READY;
#if defined(RSF_HAVE_STREAMLINE_VK)
    const int count = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, setup->runtime_directory_utf8, -1, nullptr, 0);
    if (!count) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::wstring path(static_cast<size_t>(count), 0);
    if (!MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, setup->runtime_directory_utf8, -1, path.data(), count)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!accepted(VulkanStorySlInitializeFeatures(path.c_str(), setup->project_id, 1, 0, (setup->features & RSF_SLVK_DLSS_SR) ? 1u : 0u))) return RSF_BACKEND_ERROR_LOAD_FAILED;
    bound = false; mode = interval = 0;
    tokens = {}; session = render_frame = present_frame = 0;
    status = {sizeof(status), RSF_LATENCYVK_ABI_VERSION};
    initialized = true;
    return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_SUPPORTED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" rsf_backend_result rsf_slvk_invoke(const rsf_slvk_call* call) try {
    if (!call || call->struct_size < sizeof(*call)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (call->abi_version != RSF_SLVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (!call->callback || ((call->session_id == 0) != (call->frame_id == 0))) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard lock(mutex);
    if (!initialized) return RSF_BACKEND_ERROR_NOT_READY;
#if defined(RSF_HAVE_STREAMLINE_VK)
    uintptr_t token = 0;
    if (call->frame_id) {
        const auto& slot = tokens[call->frame_id % tokens.size()];
        if (session != call->session_id || slot.frame != call->frame_id ||
            !VulkanStorySlTokenMatches(slot.token, static_cast<uint32_t>(call->frame_id))) return RSF_BACKEND_ERROR_NOT_READY;
        token = slot.token;
    }
    void* module = VulkanStorySlModule();
    return module ? call->callback(module, token, call->user) : RSF_BACKEND_ERROR_NOT_READY;
#else
    return RSF_BACKEND_ERROR_NOT_SUPPORTED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" rsf_vk_function rsf_latencyvk_instance_proc(void* instance, const char* name) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    // Resolver remains usable after slShutdown for Vulkan object destruction.
    return name ? VulkanStorySlGetInstanceProcAddr(instance, name) : nullptr;
#else
    (void)instance; (void)name; return nullptr;
#endif
}
extern "C" rsf_backend_result rsf_latencyvk_options(uint32_t requested, uint32_t limit) {
    if (requested > 2) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard lock(mutex);
    if (!initialized) return requested ? RSF_BACKEND_ERROR_NOT_SUPPORTED : RSF_BACKEND_OK;
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (!bind()) return RSF_BACKEND_ERROR_NOT_READY;
    if ((requested || limit) && !status.low_latency_available) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
#endif
    mode = requested; interval = limit;
    return RSF_BACKEND_OK;
}
extern "C" void rsf_latencyvk_cpu(const rsf_game_cpu_event* event) {
    if (!event || event->struct_size < sizeof(*event) || !event->session_id || !event->source_frame_id) return;
    std::lock_guard lock(mutex);
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (!initialized || !bind()) return;
    if (event->stage == RSF_GAME_CPU_FRAME_BEGIN) {
        if (session != event->session_id) { tokens = {}; session = event->session_id; render_frame = present_frame = 0; }
        auto& slot = tokens[event->source_frame_id % tokens.size()];
        if (slot.frame == event->source_frame_id) return;
        slot = {};
        if (accepted(VulkanStorySlBeginFrame(static_cast<uint32_t>(event->source_frame_id))))
            slot = {event->source_frame_id, VulkanStorySlCurrentFrameToken(), 0};
    } else if (event->stage == RSF_GAME_CPU_PACING) {
        auto& slot = tokens[event->source_frame_id % tokens.size()];
        if (slot.frame == event->source_frame_id && slot.token == VulkanStorySlCurrentFrameToken() &&
            VulkanStorySlTokenMatches(slot.token, static_cast<uint32_t>(event->source_frame_id)) &&
            !(slot.stages & (1u << 31))) {
            slot.stages |= 1u << 31;
            // LL2 mode belongs to the real swapchain. Refresh once per CPU
            // frame so a resized successor receives unchanged requested options.
            if (!accepted(VulkanStorySlSetReflexInterval(static_cast<int>(mode), interval))) return;
            status.mode = mode;
            if (status.low_latency_available && accepted(VulkanStorySlReflexSleep())) ++status.sleep_calls;
        }
    } else if (event->stage == RSF_GAME_CPU_SIMULATION_BEGIN) marker(event->source_frame_id, 0);
    else if (event->stage == RSF_GAME_CPU_SIMULATION_END) marker(event->source_frame_id, 1);
    else if (event->stage == RSF_GAME_CPU_FRAME_END && !event->render_expected) tokens[event->source_frame_id % tokens.size()] = {};
#endif
}
extern "C" rsf_backend_result rsf_latencyvk_input(uint64_t frame, uint32_t kind, uint32_t message) {
    std::lock_guard lock(mutex);
#if defined(RSF_HAVE_STREAMLINE_VK)
    auto& slot = tokens[frame % tokens.size()];
    if (!initialized || !bound || !frame || slot.frame != frame || !VulkanStorySlTokenMatches(slot.token, static_cast<uint32_t>(frame))) return RSF_BACKEND_ERROR_NOT_READY;
    if (kind & ~7u) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    uint32_t marker_kind = 0;
    if (message) {
        if (!status.pcl_message || message != status.pcl_message) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        marker_kind = 8;
    } else if (kind & 4u) {
        if (slot.stages & (1u << 13)) return RSF_BACKEND_OK;
        marker_kind = 13;
    } else {
        // Keyboard/mouse timing uses real input dequeue and registered PCL
        // pings. Marker 6 is deprecated; marker 13 is controller-only.
        return kind ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    if (!accepted(VulkanStorySlMarkerForToken(slot.token, marker_kind))) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    if (marker_kind == 13) slot.stages |= 1u << 13;
    ++status.marker_calls;
    return RSF_BACKEND_OK;
#else
    (void)frame; (void)kind; (void)message; return RSF_BACKEND_ERROR_NOT_SUPPORTED;
#endif
}
extern "C" void rsf_latencyvk_render(uint64_t frame) {
    std::lock_guard lock(mutex); render_frame = frame;
#if defined(RSF_HAVE_STREAMLINE_VK)
    marker(frame, 2);
#endif
}
extern "C" void rsf_latencyvk_present(uint32_t begin) {
    std::lock_guard lock(mutex);
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (begin) { present_frame = render_frame; marker(present_frame, 3); marker(present_frame, 4); }
    else { marker(present_frame, 5); present_frame = 0; render_frame = 0; }
#else
    (void)begin;
#endif
}
extern "C" rsf_backend_result rsf_latencyvk_get_status(rsf_latencyvk_status* output) {
    if (!output || output->struct_size < sizeof(*output)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (output->abi_version != RSF_LATENCYVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    std::lock_guard lock(mutex); *output = status;
    return initialized ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_NOT_READY;
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
extern "C" rsf_backend_result rsf_latencyvk_shutdown() {
    std::lock_guard lock(mutex);
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (initialized && !accepted(VulkanStorySlShutdownChecked())) return RSF_BACKEND_ERROR_FEATURE_FAILED;
#endif
    initialized = bound = false; tokens = {}; render_frame = present_frame = session = 0;
    return RSF_BACKEND_OK;
}
