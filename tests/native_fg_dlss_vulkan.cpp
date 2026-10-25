// SPDX-License-Identifier: GPL-3.0-only
// Synthetic SDK/WSI boundary. No NVIDIA runtime or Vulkan device is exercised.
#include "../runtime/orchestrator/src/native_fg_dlss_vulkan.cpp"
#include "../runtime/orchestrator/src/native_fg_dlss_vulkan_capture.cpp"
#include <cstdio>
#include <cstdlib>

namespace {
uint64_t lease_session = 7, lease_frame = 42;
uint32_t sdk_token = 42;
uint32_t on_calls = 0, off_calls = 0, present_calls = 0, fence_submits = 0;
uint32_t sdk_presented = 0;
bool refuse_options = false, refuse_submit = false;
uint64_t worker_timeout_epoch = 0;
bool worker_timeout = false;
bool refuse_inputs_wait = false;
uint32_t inputs_waits = 0;
uint64_t expected_inputs_value = 9;
void require(bool condition, const char* message) {
    if (!condition) { std::fprintf(stderr, "%s\n", message); std::exit(1); }
}
sl::Result options(const sl::ViewportHandle& view, const sl::DLSSGOptions& value) {
    require(static_cast<uint32_t>(view) == 0, "FG uses root viewport zero");
    require(value.queueParallelismMode == sl::DLSSGQueueParallelismMode::eBlockNoClientQueues,
        "SDK never queues a future timeline wait ahead of client binary waits");
    require(value.enableUserInterfaceRecomposition == sl::Boolean::eFalse, "missing UI producer does not enable recomposition");
    require((static_cast<uint32_t>(value.flags) & static_cast<uint32_t>(sl::DLSSGFlags::eRetainResourcesWhenOff)) != 0,
        "Off never releases resources before CPU presenter retirement");
    if (refuse_options) return sl::Result::eErrorInvalidParameter;
    if (value.mode == sl::DLSSGMode::eOn) {
        ++on_calls; require(value.numFramesToGenerate == 3, "MFG forwards generated count excluding real frame");
    } else ++off_calls;
    return sl::Result::eOk;
}
sl::Result state(const sl::ViewportHandle&, sl::DLSSGState& value, const sl::DLSSGOptions*) {
    value.numFramesToGenerateMax = 3; value.minWidthOrHeight = 128;
    value.numFramesActuallyPresented = sdk_presented; sdk_presented = 0;
    value.inputsProcessingCompletionFence = reinterpret_cast<void*>(0xc9);
    value.lastPresentInputsProcessingCompletionFenceValue = 9;
    return sl::Result::eOk;
}
VkResult VKAPI_PTR reset_fences(VkDevice, uint32_t, const VkFence*) { return VK_SUCCESS; }
VkResult VKAPI_PTR wait_fences(VkDevice, uint32_t, const VkFence*, VkBool32, uint64_t) { return VK_SUCCESS; }
void VKAPI_PTR destroy_fence(VkDevice, VkFence, const VkAllocationCallbacks*) {}
VkResult VKAPI_PTR present(VkQueue, const VkPresentInfoKHR*) { ++present_calls; return VK_SUCCESS; }
VkResult VKAPI_PTR idle(VkDevice) { if (worker_timeout) ++worker_timeout_epoch; return VK_SUCCESS; }
VkResult VKAPI_PTR wait_semaphores(VkDevice, const VkSemaphoreWaitInfo* info, uint64_t timeout) {
    require(info->semaphoreCount == 1 && info->pSemaphores[0] == reinterpret_cast<VkSemaphore>(0xc9) &&
        info->pValues[0] == expected_inputs_value && timeout == 15000000000ull, "waits the captured SDK completion value, not a source frame ID");
    ++inputs_waits; return refuse_inputs_wait ? VK_TIMEOUT : VK_SUCCESS;
}
void queue_lock(void*) {}
int32_t submit(void*, uint32_t count, const void* commands, uint64_t fence) {
    require(count == 0 && !commands && fence, "completion fences follow real host submissions");
    ++fence_submits; return refuse_submit ? VK_ERROR_DEVICE_LOST : VK_SUCCESS;
}
void submitted_slot(rsf_dlssgvk& self, uint64_t frame, void* prepare, void* hudless) {
    auto& slot = self.slots[frame % self.slots.size()]; slot = {};
    slot.recording_sequence = ++self.recording_sequence;
    slot.record.frame_id = frame; slot.record.session_id = lease_session;
    slot.prepared = slot.hudless = true;
    slot.recorded_prepare = slot.recorded_hudless = true;
    slot.prepare_commands = prepare; slot.hudless_commands = hudless;
    slot.source_completion = reinterpret_cast<VkFence>(1); slot.consumer_completion = reinterpret_cast<VkFence>(2);
    self.latest = frame;
}
}
extern "C" rsf_backend_result rsf_slvk_invoke(const rsf_slvk_call* call) {
    if (call->session_id && (call->session_id != lease_session || call->frame_id != lease_frame)) return RSF_BACKEND_ERROR_NOT_READY;
    return call->callback(reinterpret_cast<void*>(1), call->frame_id ? reinterpret_cast<uintptr_t>(&sdk_token) : 0, call->user);
}
extern "C" uint64_t VulkanStorySlWorkerTimeoutEpoch() { return worker_timeout_epoch; }
extern "C" int32_t rsf_latencyvk_wsi_call(rsf_vk_wsi_call callback, void* user) { return callback(user); }
extern "C" int32_t rsf_latencyvk_queue_present(rsf_vk_function callback, void* queue, const void* info) {
    return reinterpret_cast<PFN_vkQueuePresentKHR>(callback)(static_cast<VkQueue>(queue), static_cast<const VkPresentInfoKHR*>(info));
}
int main() {
    SetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", nullptr);
    require(rsf_dlssgvk_create(nullptr, nullptr, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "null create refused");
    require(rsf_dlssgvk_prepare(nullptr, nullptr, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "null recording refused");
    require(rsf_dlssgvk_destroy(nullptr) == RSF_BACKEND_OK, "null destruction idempotent");
    rsf_dlssgvk self{}; self.setup.graphics.device = reinterpret_cast<void*>(1);
    self.setup.queues[0] = reinterpret_cast<void*>(2); self.setup.graphics_submit = submit;
    self.width = 1280; self.height = 720; self.backbuffers = 3;
    self.chain = reinterpret_cast<VkSwapchainKHR>(3); self.format = VK_FORMAT_B8G8R8A8_UNORM;
    self.options = options; self.state = state; self.present = present;
    self.reset_fences = reset_fences; self.wait_fences = wait_fences; self.destroy_fence = destroy_fence;
    self.wait_semaphores = wait_semaphores;
    self.status.max_generated_frames = 3;
    uint32_t image_layout = 0;
    require(rsf_dlssgvk_image_layout(&self, &image_layout) == RSF_BACKEND_OK && image_layout == VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL &&
        rsf_dlssgvk_image_layout(&self, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT,
        "DLSS game-facing proxy images require transfer source layout even while interpolation is Off");
    require(rsf_dlssgvk_enable(&self, 1, 4) == RSF_BACKEND_ERROR_NOT_SUPPORTED && !self.requested,
        "unsupported MFG count refuses without changing request");
    require(rsf_dlssgvk_enable(&self, 1, 0) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "zero interpolation count refused");
    require(rsf_dlssgvk_enable(&self, 1, 3) == RSF_BACKEND_OK, "4x MFG request accepted");
    VkPresentInfoKHR info{}; info.sType = VK_STRUCTURE_TYPE_PRESENT_INFO_KHR; info.swapchainCount = 1; info.pSwapchains = &self.chain;
    auto* prepare = reinterpret_cast<void*>(10); auto* hudless = reinterpret_cast<void*>(11);
    submitted_slot(self, 42, prepare, hudless);
    require(rsf_dlssgvk_present(&self, &info) == VK_SUCCESS && !on_calls && !self.status.presentation.enabled,
        "recording without submission never enables FG");
    submitted_slot(self, 43, prepare, hudless); lease_frame = 43;
    rsf_dlssgvk_submitted(&self, &prepare, 1);
    require(fence_submits == 1 && self.slots[43 % 6].source_pending, "prepare-only submission gets completion fence");
    require(rsf_dlssgvk_present(&self, &info) == VK_SUCCESS && !on_calls,
        "prepare-only submitted frame presents ordinarily");
    submitted_slot(self, 44, prepare, hudless); lease_frame = 44;
    void* lists[] = {prepare, hudless}; rsf_dlssgvk_submitted(&self, lists, 2);
    const auto previous_submits = fence_submits;
    rsf_dlssgvk_submitted(&self, lists, 2);
    require(fence_submits == previous_submits, "duplicate submission notification does not reset live fence");
    sdk_presented = 4;
    require(rsf_dlssgvk_present(&self, &info) == VK_SUCCESS && on_calls == 1 && self.status.presentation.enabled,
        "matching fully submitted source enables requested MFG");
    require(self.status.presentation.source_frame == 44 && self.status.presentation.last_present_count == 4 &&
        !self.status.presentation.generated_callbacks, "real SDK counts and source identity are distinct from callback counts");
    auto& in_flight = self.slots[44 % 6];
    require(in_flight.sdk_inputs_pending && in_flight.sdk_inputs_value == 9, "accepted present retains borrowed SDK completion lease per source");
    refuse_inputs_wait = true;
    require(wait_inputs(self, in_flight) == RSF_BACKEND_ERROR_NOT_READY && in_flight.sdk_inputs_pending,
        "pending SDK inputs cannot be overwritten after positive timeout");
    refuse_inputs_wait = false;
    require(wait_inputs(self, in_flight) == RSF_BACKEND_OK && !in_flight.sdk_inputs_pending && inputs_waits == 2,
        "successful SDK completion permits private input reuse");
    in_flight.sdk_inputs_pending = true; in_flight.sdk_inputs_value = expected_inputs_value = 0;
    require(wait_inputs(self, in_flight) == RSF_BACKEND_OK && !in_flight.sdk_inputs_pending,
        "SDK timeline value zero is a valid captured completion, not a missing lease");
    in_flight.sdk_inputs_value = expected_inputs_value = 9;
    require(rsf_dlssgvk_present(&self, &info) == VK_SUCCESS && off_calls == 1 && !self.status.presentation.enabled,
        "duplicate source disables interpolation while preserving ordinary Present");
    submitted_slot(self, 45, prepare, hudless); lease_frame = 46; rsf_dlssgvk_submitted(&self, lists, 2);
    require(rsf_dlssgvk_present(&self, &info) == VK_SUCCESS && on_calls == 1 && !self.status.presentation.enabled,
        "expired original token never replaced with newer CPU token");
    submitted_slot(self, 47, prepare, hudless); lease_frame = 47; refuse_submit = true;
    rsf_dlssgvk_submitted(&self, lists, 2); refuse_submit = false;
    require(rsf_dlssgvk_present(&self, &info) == VK_SUCCESS && on_calls == 1, "unknown submission completion refuses interpolation");
    submitted_slot(self, 48, prepare, hudless); lease_frame = 48; rsf_dlssgvk_submitted(&self, lists, 2);
    refuse_options = true;
    require(rsf_dlssgvk_present(&self, &info) == VK_SUCCESS && !self.status.presentation.enabled,
        "failed enable retains ordinary native presentation");
    refuse_options = false;
    lease_session = 8; lease_frame = 1;
    submitted_slot(self, 1, prepare, hudless); rsf_dlssgvk_submitted(&self, lists, 2);
    require(rsf_dlssgvk_present(&self, &info) == VK_SUCCESS && self.status.presentation.enabled && self.status.presentation.source_frame == 1,
        "new session restarting source IDs is ordered by local recording lease, not old source number");
    VkSwapchainKHR foreign = reinterpret_cast<VkSwapchainKHR>(99); info.pSwapchains = &foreign;
    const auto previous_presents = present_calls;
    require(rsf_dlssgvk_present(&self, &info) == VK_ERROR_INITIALIZATION_FAILED && present_calls == previous_presents,
        "foreign chain never reaches owner WSI");
    rsf_dlssgvk_status status{}; status.struct_size = sizeof(status); status.abi_version = 99;
    require(rsf_dlssgvk_get_status(&self, &status) == RSF_BACKEND_ERROR_ABI_MISMATCH, "status ABI refused");
    status.abi_version = RSF_DLSSGVK_ABI_VERSION;
    require(rsf_dlssgvk_get_status(&self, &status) == RSF_BACKEND_OK && status.requested_generated_frames == 3,
        "status preserves independent requested count");
    self.setup.lock_queues = queue_lock; self.setup.unlock_queues = queue_lock; self.idle = idle;
    worker_timeout = true;
    require(rsf_dlssgvk_drain(&self) == RSF_BACKEND_ERROR_NOT_READY && self.chain && self.requested,
        "worker flush timeout preserves chain, resources and request despite successful Vulkan idle result");
    worker_timeout = false;
    refuse_inputs_wait = true;
    require(rsf_dlssgvk_drain(&self) == RSF_BACKEND_ERROR_NOT_READY && self.chain,
        "GPU idle alone cannot discard a pending borrowed SDK completion lease");
    refuse_inputs_wait = false;
    require(rsf_dlssgvk_drain(&self) == RSF_BACKEND_OK && self.requested && self.count == 3,
        "later successful proxy drain permits retirement without changing requested MFG");
    lease_frame = 2; submitted_slot(self, 2, prepare, hudless);
    auto& rejected = self.slots[2]; rejected.prepared = rejected.hudless = false;
    const auto before_rejected = fence_submits;
    rsf_dlssgvk_submitted(&self, lists, 2);
    require(fence_submits == before_rejected + 1 && rejected.source_pending && rejected.submitted_prepare && rejected.submitted_hudless,
        "recorded copies are fenced even when subsequent SDK tagging rejects the frame");
    info.pSwapchains = &self.chain;
    require(rsf_dlssgvk_present(&self, &info) == VK_SUCCESS && !self.status.presentation.enabled,
        "fenced rejected tags never become accepted interpolation inputs");
    std::puts("Synthetic DLSS Vulkan FG identity, MFG and WSI checks passed");
}
