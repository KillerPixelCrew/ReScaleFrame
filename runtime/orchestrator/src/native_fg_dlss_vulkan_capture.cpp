// SPDX-License-Identifier: MIT
#include "native_fg_dlss_vulkan_capture.h"
#include "native_fg_dlss_vulkan_capture_ledger.h"
#include "../../vulkan/streamline/present-capture.h"
#include <cstring>

#if defined(RSF_HAVE_STREAMLINE_VK)
#define VK_NO_PROTOTYPES
#include <windows.h>
#include <vulkan/vulkan.h>
#include "../../backends/common/frame_capture_vk.h"
#include <fstream>
#include <map>
#include <mutex>
#include <string>
#include <vector>

namespace rsf {
struct DlssPresentCapture::State {
    struct Commands { VkCommandPool pool = VK_NULL_HANDLE; VkCommandBuffer commands = VK_NULL_HANDLE; VkFence fence = VK_NULL_HANDLE; };
    rsf_fgvk_setup setup{};
    rsf_sl_set_present_capture registration = nullptr;
    bool registered = false;
    FrameCaptureVk capture;
    std::mutex guard;
    rsf_dlss_capture_ledger* ledger = nullptr;
    std::map<uint32_t, Commands> command_families;
    std::filesystem::path directory;
    PFN_vkCreateCommandPool create_pool = nullptr; PFN_vkDestroyCommandPool destroy_pool = nullptr;
    PFN_vkResetCommandPool reset_pool = nullptr; PFN_vkAllocateCommandBuffers allocate_commands = nullptr;
    PFN_vkBeginCommandBuffer begin = nullptr; PFN_vkEndCommandBuffer end = nullptr;
    PFN_vkCreateFence create_fence = nullptr; PFN_vkDestroyFence destroy_fence = nullptr;
    PFN_vkResetFences reset_fences = nullptr; PFN_vkWaitForFences wait_fences = nullptr;
    ~State() {
        rsf_core_dlss_capture_destroy(ledger);
        const auto device = static_cast<VkDevice>(setup.graphics.device);
        for (auto& [family, command] : command_families) {
            (void)family;
            if (command.fence) destroy_fence(device, command.fence, nullptr);
            if (command.pool) destroy_pool(device, command.pool, nullptr);
        }
    }
    void failure(uint64_t source, const char* reason, int32_t result) noexcept try {
        std::ofstream(directory / "dlss-present-capture-errors.log", std::ios::app) << source << ' ' << reason << ' ' << result << '\n';
    } catch (...) {}
    bool commands(uint32_t family, Commands*& result) {
        auto& value = command_families[family];
        const auto device = static_cast<VkDevice>(setup.graphics.device);
        if (rsf_core_dlss_capture_pending(ledger, family)) {
            if (wait_fences(device, 1, &value.fence, VK_TRUE, 15000000000ull) != VK_SUCCESS) return false;
            rsf_core_dlss_capture_completed(ledger, family);
        }
        if (!value.pool) {
            VkCommandPoolCreateInfo info{}; info.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
            info.queueFamilyIndex = family; info.flags = VK_COMMAND_POOL_CREATE_TRANSIENT_BIT;
            if (create_pool(device, &info, nullptr, &value.pool) != VK_SUCCESS) return false;
        }
        if (!value.commands) {
            VkCommandBufferAllocateInfo info{}; info.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
            info.commandPool = value.pool; info.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY; info.commandBufferCount = 1;
            if (allocate_commands(device, &info, &value.commands) != VK_SUCCESS) return false;
        }
        if (!value.fence) {
            VkFenceCreateInfo info{}; info.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO;
            if (create_fence(device, &info, nullptr, &value.fence) != VK_SUCCESS) return false;
        }
        if (reset_pool(device, value.pool, 0) != VK_SUCCESS || reset_fences(device, 1, &value.fence) != VK_SUCCESS) return false;
        result = &value; return true;
    }
    int32_t observe(const rsf_sl_present_capture_packet& packet) {
        const auto present = reinterpret_cast<PFN_vkQueuePresentKHR>(packet.native_present);
        const auto& info = *static_cast<const VkPresentInfoKHR*>(packet.present_info);
        const auto queue = static_cast<VkQueue>(packet.queue);
        const auto native_images = reinterpret_cast<PFN_vkGetSwapchainImagesKHR>(packet.native_images);
        const auto native_submit = reinterpret_cast<PFN_vkQueueSubmit>(packet.native_submit);
        using WsiCall = int32_t (*)(int32_t (*)(void*), void*);
        const auto wsi = reinterpret_cast<WsiCall>(packet.native_wsi_call);
        auto real_present = [&](const VkPresentInfoKHR& value) -> int32_t {
            struct Call { PFN_vkQueuePresentKHR next; VkQueue queue; const VkPresentInfoKHR* info; } call{present, queue, &value};
            return wsi([](void* user) -> int32_t { const auto& item = *static_cast<Call*>(user); return item.next(item.queue, item.info); }, &call);
        };
        auto real_images = [&](VkSwapchainKHR chain, uint32_t* count, VkImage* output) -> int32_t {
            struct Call { PFN_vkGetSwapchainImagesKHR next; VkDevice device; VkSwapchainKHR chain; uint32_t* count; VkImage* images; };
            Call call{native_images, static_cast<VkDevice>(packet.device), chain, count, output};
            return wsi([](void* user) -> int32_t { const auto& item = *static_cast<Call*>(user); return item.next(item.device, item.chain, item.count, item.images); }, &call);
        };
        // The common callback is synchronous and already owns its SDK queue.
        // Never acquire a session/PCL/host-queue lock from this presenter thread.
        std::lock_guard lock(guard);
        rsf_dlss_capture_source source{}, observed{};
        const bool found = rsf_core_dlss_capture_source(ledger, packet.source_frame, &source) != 0;
        // Preserve raw producer facts even when the SDK boundary does not map
        // to a retained source. Otherwise a mapping bug can hide generated
        // presents and make a real-only capture look like disabled generation.
        if (rsf_core_dlss_capture_observed(ledger, &observed) && capture.wants(observed.record.frame_id)) try {
            const auto& sdk = observed.sdk;
            std::ofstream trace(directory / "dlss-present-boundaries.jsonl", std::ios::app);
            trace << "{\"capture_anchor_frame\":" << observed.record.frame_id
                << ",\"capture_anchor_session\":" << observed.record.session_id << ",\"sdk_source_frame\":" << packet.source_frame
                << ",\"sdk_present_frame\":" << packet.present_frame << ",\"sdk_generated_index\":" << packet.generated_index
                << ",\"boundary_valid\":" << (packet.boundary_valid ? "true" : "false")
                << ",\"source_found\":" << (found ? "true" : "false")
                << ",\"configured_on\":" << (observed.configured ? "true" : "false")
                << ",\"requested_generated_count\":" << observed.generated_count
                << ",\"sdk_status\":" << sdk.sdk_status << ",\"sdk_result\":" << sdk.sdk_result
                << ",\"sdk_actual_presents\":" << sdk.presentation.last_present_count
                << ",\"max_generated\":" << sdk.max_generated_frames << ",\"min_dimension\":" << sdk.min_dimension
                << ",\"vsync_supported\":" << sdk.vsync_supported << ",\"dynamic_mfg_supported\":" << sdk.dynamic_mfg_supported
                << ",\"state_sample\":\"previous_sdk_query\"}\n";
        } catch (...) {}
        if (!packet.boundary_valid || !found || !capture.wants(source.record.frame_id) ||
            !rsf_core_dlss_capture_can_record(ledger)) return real_present(info);
        const auto& chain_info = *static_cast<const VkSwapchainCreateInfoKHR*>(packet.swapchain_info);
        if (!(chain_info.imageUsage & VK_IMAGE_USAGE_TRANSFER_SRC_BIT) || info.swapchainCount != 1 || !info.pSwapchains || !info.pImageIndices ||
            chain_info.imageArrayLayers != 1 || (chain_info.imageFormat != VK_FORMAT_B8G8R8A8_UNORM && chain_info.imageFormat != VK_FORMAT_R8G8B8A8_UNORM)) {
            failure(packet.source_frame, "real-chain-copy-contract", VK_ERROR_FORMAT_NOT_SUPPORTED); return real_present(info);
        }
        const auto device = static_cast<VkDevice>(packet.device);
        uint32_t count = 0;
        auto result = real_images(info.pSwapchains[0], &count, nullptr);
        if (result != VK_SUCCESS || info.pImageIndices[0] >= count) {
            failure(packet.source_frame, "real-images-count", result); return real_present(info);
        }
        std::vector<VkImage> actual_images(count);
        result = real_images(info.pSwapchains[0], &count, actual_images.data());
        if (result != VK_SUCCESS || count > actual_images.size() || info.pImageIndices[0] >= count ||
            !actual_images[info.pImageIndices[0]]) {
            failure(packet.source_frame, "real-images", result != VK_SUCCESS ? result : VK_ERROR_INITIALIZATION_FAILED);
            return real_present(info);
        }
        Commands* work = nullptr;
        if (!commands(packet.queue_family, work)) { failure(packet.source_frame, "capture-commands", VK_ERROR_INITIALIZATION_FAILED); return real_present(info); }
        // Allocate host metadata before recording can retain a readback.
        std::vector<VkPipelineStageFlags> stages(info.waitSemaphoreCount, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT);
        const auto role = packet.generated_index >= 0 ? "fg-generated-present-" + std::to_string(packet.generated_index) : "fg-real-present";
        VkCommandBufferBeginInfo begin_info{}; begin_info.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
        begin_info.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
        result = begin(work->commands, &begin_info);
        if (result != VK_SUCCESS) { failure(packet.source_frame, "begin", result); return real_present(info); }
        const auto& identity = source.record;
        const rsf_srvk_image image{packet.device, reinterpret_cast<uint64_t>(actual_images[info.pImageIndices[0]]), uint32_t(chain_info.imageFormat),
            chain_info.imageExtent.width, chain_info.imageExtent.height, VK_IMAGE_LAYOUT_PRESENT_SRC_KHR, 1};
        const auto pending_before = capture.pending_count();
        const bool recorded = capture.record(work->commands, image, identity, role.c_str(), identity.flags & RSF_FRAME_FLAG_RESET, false);
        result = end(work->commands);
        if (!recorded || result != VK_SUCCESS) {
            if (capture.pending_count() > pending_before) rsf_core_dlss_capture_unsubmitted(ledger);
            failure(packet.source_frame, "record", result); return real_present(info);
        }
        VkSubmitInfo submission{}; submission.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO;
        submission.waitSemaphoreCount = info.waitSemaphoreCount; submission.pWaitSemaphores = info.pWaitSemaphores;
        submission.pWaitDstStageMask = stages.data(); submission.commandBufferCount = 1; submission.pCommandBuffers = &work->commands;
        result = native_submit(queue, 1, &submission, work->fence);
        if (result != VK_SUCCESS) {
            if (capture.pending_count() > pending_before) rsf_core_dlss_capture_unsubmitted(ledger);
            failure(packet.source_frame, "submit", result); return result;
        }
        rsf_core_dlss_capture_submitted(ledger, packet.queue_family);
        result = wait_fences(device, 1, &work->fence, VK_TRUE, 15000000000ull);
        if (result != VK_SUCCESS) { failure(packet.source_frame, "wait", result); return result > 0 ? VK_ERROR_INITIALIZATION_FAILED : result; }
        rsf_core_dlss_capture_completed(ledger, packet.queue_family);
        // guard serializes all dedicated capture recordings. Every earlier
        // submitted copy must also have a successful own-fence wait, including
        // other queue families after a previous timeout. No global idle or SDK
        // lock is acquired here. Legacy readbacks have no core completion ticket,
        // so flush(true) would deliberately leave their pixels unwritten.
        const bool readbacks_ready = rsf_core_dlss_capture_can_flush(ledger) != 0;
        const bool pixels_flushed = readbacks_ready && capture.flush(false);
        if (readbacks_ready && !pixels_flushed)
            failure(packet.source_frame, "completed-readback-flush", VK_ERROR_INITIALIZATION_FAILED);
        // Diagnostic readback is intentionally synchronous and bounded. Its GPU
        // fence completes the original binary waits before native Present, so
        // no replacement semaphore or image-index reuse assumption is needed.
        VkPresentInfoKHR completed = info; completed.waitSemaphoreCount = 0; completed.pWaitSemaphores = nullptr;
        result = real_present(completed);
        try {
            std::ofstream metadata(directory / "dlss-present-capture.jsonl", std::ios::app);
            metadata << "{\"sequence\":" << rsf_core_dlss_capture_sequence(ledger) << ",\"session\":" << identity.session_id << ",\"frame\":" << identity.frame_id
            << ",\"view\":" << identity.view_id << ",\"resource_generation\":" << identity.resource_generation
            << ",\"sdk_source_frame\":" << packet.source_frame << ",\"sdk_present_frame\":" << packet.present_frame
            << ",\"sdk_generated_index\":" << packet.generated_index << ",\"requested_generated_count\":" << source.generated_count
            << ",\"generated\":" << (packet.generated_index >= 0 ? "true" : "false") << ",\"real_image_index\":" << info.pImageIndices[0]
            << ",\"real_swapchain\":" << reinterpret_cast<uint64_t>(info.pSwapchains[0]) << ",\"present_result\":" << result
            << ",\"legacy_flush_attempted\":" << (readbacks_ready ? "true" : "false")
            << ",\"legacy_flush_succeeded\":" << (pixels_flushed ? "true" : "false")
            << ",\"readback_completion_proof\":" << (readbacks_ready ? "\"dedicated-capture-fences\"" : "null")
                << ",\"role\":\"" << role << "\",\"fraction_established\":false}\n";
        } catch (...) {}
        return result;
    }
    static int32_t callback(void* user, const rsf_sl_present_capture_packet* packet) noexcept try {
        if (!user || !packet || packet->struct_size < sizeof(*packet) || packet->abi_version != RSF_SL_PRESENT_CAPTURE_ABI_VERSION ||
            !packet->device || !packet->queue || !packet->present_info || !packet->swapchain_info || !packet->native_present ||
            !packet->native_images || !packet->native_submit || !packet->native_wsi_call) return VK_ERROR_INITIALIZATION_FAILED;
        return static_cast<State*>(user)->observe(*packet);
    } catch (...) { return VK_ERROR_INITIALIZATION_FAILED; }
};
DlssPresentCapture::DlssPresentCapture() = default;
DlssPresentCapture::~DlssPresentCapture() = default;
rsf_backend_result DlssPresentCapture::initialize(const rsf_fgvk_setup& setup) {
    auto next = std::make_unique<State>();
    if (!next->capture.enabled()) return RSF_BACKEND_OK;
    next->ledger = rsf_core_dlss_capture_create();
    if (!next->ledger) return RSF_BACKEND_ERROR_INIT_FAILED;
    next->setup = setup;
    wchar_t directory[32768]{};
    const auto length = GetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", directory, 32768);
    if (!length || length >= 32768 || !setup.graphics.runtime_directory_utf8) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    next->directory = directory;
    std::u8string runtime_directory(std::strlen(setup.graphics.runtime_directory_utf8), u8'\0');
    std::memcpy(runtime_directory.data(), setup.graphics.runtime_directory_utf8, runtime_directory.size());
    const auto common = std::filesystem::path(runtime_directory) / L"sl.common.dll";
    const auto module = GetModuleHandleW(common.c_str());
    if (!module) return RSF_BACKEND_ERROR_NOT_READY;
    next->registration = reinterpret_cast<rsf_sl_set_present_capture>(GetProcAddress(module, "rsfSlSetPresentCapture"));
    if (!next->registration) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
#define RSF_CAPTURE_LOAD(member, name) next->member = reinterpret_cast<PFN_##name>(setup.graphics.device_proc(setup.graphics.device, #name)); if (!next->member) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT
    RSF_CAPTURE_LOAD(create_pool, vkCreateCommandPool); RSF_CAPTURE_LOAD(destroy_pool, vkDestroyCommandPool);
    RSF_CAPTURE_LOAD(reset_pool, vkResetCommandPool); RSF_CAPTURE_LOAD(allocate_commands, vkAllocateCommandBuffers);
    RSF_CAPTURE_LOAD(begin, vkBeginCommandBuffer); RSF_CAPTURE_LOAD(end, vkEndCommandBuffer);
    RSF_CAPTURE_LOAD(create_fence, vkCreateFence); RSF_CAPTURE_LOAD(destroy_fence, vkDestroyFence);
    RSF_CAPTURE_LOAD(reset_fences, vkResetFences); RSF_CAPTURE_LOAD(wait_fences, vkWaitForFences);
#undef RSF_CAPTURE_LOAD
    const auto properties = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(setup.graphics.instance_proc(setup.graphics.instance, "vkGetPhysicalDeviceMemoryProperties"));
    if (!properties) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    VkPhysicalDeviceMemoryProperties memory{}; properties(static_cast<VkPhysicalDevice>(setup.graphics.physical_device), &memory);
    next->capture.set_provider(0, 1, true);
    if (!next->capture.initialize(static_cast<VkDevice>(setup.graphics.device), setup.graphics.device_proc, memory)) return RSF_BACKEND_ERROR_INIT_FAILED;
    const rsf_sl_present_capture_registration registration{sizeof(registration), RSF_SL_PRESENT_CAPTURE_ABI_VERSION, setup.graphics.device, State::callback, next.get()};
    if (next->registration(&registration) != VK_SUCCESS) return RSF_BACKEND_ERROR_NOT_READY;
    next->registered = true; state = std::move(next); return RSF_BACKEND_OK;
}
void DlssPresentCapture::remember(const rsf_frame_record& record, uint32_t count, const rsf_dlssgvk_status& sdk, bool configured) {
    if (!state) return;
    const bool wanted = state->capture.wants(record.frame_id);
    std::lock_guard lock(state->guard);
    const rsf_dlss_capture_source source{record, sdk, count, configured ? 1u : 0u};
    const auto result = rsf_core_dlss_capture_remember(state->ledger, wanted ? &source : nullptr);
    if (result) state->failure(record.frame_id,
        result == 1 ? "ambiguous-source-identity" : "source-outside-sdk-token-range", VK_ERROR_INITIALIZATION_FAILED);
}
void DlssPresentCapture::flush() {
    if (!state) return;
    std::lock_guard lock(state->guard);
    rsf_core_dlss_capture_drained(state->ledger);
    if (rsf_core_dlss_capture_can_flush(state->ledger)) (void)state->capture.flush();
}
rsf_backend_result DlssPresentCapture::detach() {
    if (!state) return RSF_BACKEND_OK;
    if (state->registered) {
        const rsf_sl_present_capture_registration registration{sizeof(registration), RSF_SL_PRESENT_CAPTURE_ABI_VERSION, state->setup.graphics.device, nullptr, state.get()};
        if (state->registration(&registration) != VK_SUCCESS) return RSF_BACKEND_ERROR_NOT_READY;
        state->registered = false;
    }
    return RSF_BACKEND_OK;
}
}
#else
namespace rsf {
struct DlssPresentCapture::State {};
DlssPresentCapture::DlssPresentCapture() = default;
DlssPresentCapture::~DlssPresentCapture() = default;
rsf_backend_result DlssPresentCapture::initialize(const rsf_fgvk_setup&) { return RSF_BACKEND_OK; }
void DlssPresentCapture::remember(const rsf_frame_record&, uint32_t, const rsf_dlssgvk_status&, bool) {}
void DlssPresentCapture::flush() {}
rsf_backend_result DlssPresentCapture::detach() { return RSF_BACKEND_OK; }
}
#endif
