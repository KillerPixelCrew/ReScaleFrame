// SPDX-License-Identifier: GPL-3.0-only
// Synthetic FFX presenter lifecycle. No vendor DLL, Vulkan device or GPU work.
#include "../runtime/orchestrator/src/native_fg_vulkan.cpp"
#include <cstdio>
#include <cstdlib>

namespace {
void require(bool value, const char* message) {
    if (!value) { std::fprintf(stderr, "%s\n", message); std::exit(1); }
}
rsf_ffxgvk* active = nullptr;
bool configured_enabled = false;
uint32_t configuration_calls = 0, activation_changes = 0, dispatch_calls = 0, source_fences = 0;
bool expected_reset = false;
ffxReturnCode_t configure(ffxContext*, const ffxConfigureDescHeader* header) {
    const auto& request = *reinterpret_cast<const ffxConfigureDescFrameGeneration*>(header);
    require(request.header.type == FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION && request.frameID == active->latest,
        "each source updates the SDK frame identity");
    const bool enabled = request.frameGenerationEnabled;
    if (configured_enabled != enabled) ++activation_changes;
    configured_enabled = enabled; ++configuration_calls;
    if (enabled) {
        require(request.frameGenerationCallback && request.frameGenerationCallbackUserContext == active && request.HUDLessColor.resource,
            "enabled presenter receives its callback and leased HUD-less input");
    }
    return FFX_API_RETURN_OK;
}
ffxReturnCode_t dispatch(ffxContext*, const ffxDispatchDescHeader* header) {
    const auto& request = *reinterpret_cast<const ffxDispatchDescFrameGeneration*>(header);
    require(request.reset == expected_reset, "camera reset reaches SDK dispatch without changing activation");
    ++dispatch_calls; return FFX_API_RETURN_OK;
}
VkResult VKAPI_PTR present(VkQueue, const VkPresentInfoKHR*) {
    if (configured_enabled) {
        ffxDispatchDescFrameGeneration request{}; request.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION;
        request.frameID = active->latest;
        require(generate(&request, active) == FFX_API_RETURN_OK, "SDK callback accepts the submitted source frame");
    }
    return VK_SUCCESS;
}
VkResult VKAPI_PTR reset_fences(VkDevice, uint32_t, const VkFence*) { return VK_SUCCESS; }
int32_t submit(void*, uint32_t count, const void* commands, uint64_t fence) {
    require(!count && !commands && fence, "completion fence follows presentation work"); ++source_fences; return VK_SUCCESS;
}
void record(rsf_ffxgvk& self, uint64_t frame, uint32_t flags) {
    self.latest = frame;
    auto& slot = self.slots[frame % self.slots.size()]; slot = {};
    slot.recording_sequence = ++self.recording_sequence;
    slot.record.frame_id = frame; slot.record.flags = flags;
    slot.prepared = slot.hudless = slot.submitted_prepare = slot.submitted_hudless = true;
    slot.consumer_completion = reinterpret_cast<VkFence>(uintptr_t(21));
    slot.color.image = reinterpret_cast<VkImage>(uintptr_t(22));
    expected_reset = (flags & RSF_FRAME_FLAG_RESET) != 0;
}
}
extern "C" int32_t rsf_slvk_without_dlssg(int32_t (*callback)(void*), void* user) { return callback(user); }
extern "C" int32_t rsf_latencyvk_wsi_call(rsf_vk_wsi_call callback, void* user) { return callback(user); }
extern "C" int32_t rsf_latencyvk_queue_present(rsf_vk_function callback, void* queue, const void* info) {
    return reinterpret_cast<PFN_vkQueuePresentKHR>(callback)(static_cast<VkQueue>(queue), static_cast<const VkPresentInfoKHR*>(info));
}

int main() {
    SetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", nullptr);
    SetEnvironmentVariableW(L"RSF_VULKAN_TIMING_FILE", nullptr);
    rsf_ffxgvk self{}; active = &self;
    self.setup.graphics.device = reinterpret_cast<void*>(uintptr_t(1));
    self.setup.queues[0] = reinterpret_cast<void*>(uintptr_t(2)); self.setup.graphics_submit = submit;
    self.chain = reinterpret_cast<VkSwapchainKHR>(uintptr_t(3)); self.width = 640; self.height = 360;
    self.configure = configure; self.dispatch = dispatch; self.reset_fences = reset_fences;
    self.functions.pOutQueuePresentKHR = present;
    VkPresentInfoKHR info{}; info.sType = VK_STRUCTURE_TYPE_PRESENT_INFO_KHR; info.swapchainCount = 1; info.pSwapchains = &self.chain;
    require(rsf_ffxgvk_enable(&self, 1) == RSF_BACKEND_OK, "explicit FG enable is accepted");
    for (uint64_t frame = 1; frame <= 4; ++frame) {
        record(self, frame, frame % 2 ? RSF_FRAME_FLAG_RESET : 0);
        require(rsf_ffxgvk_present(&self, &info) == VK_SUCCESS && self.status.enabled && self.configured,
            "submitted reset and ordinary frames keep the presenter active");
    }
    require(configuration_calls == 4 && activation_changes == 1 && dispatch_calls == 4 && source_fences == 4,
        "camera resets update per-frame SDK inputs without presenter reconfiguration");
    require(rsf_ffxgvk_enable(&self, 0) == RSF_BACKEND_OK, "explicit FG disable is accepted");
    record(self, 5, 0);
    require(rsf_ffxgvk_present(&self, &info) == VK_SUCCESS && !self.configured && !self.status.enabled &&
        activation_changes == 2 && dispatch_calls == 4, "explicit disable still retires presenter activation");
    require(rsf_ffxgvk_enable(&self, 1) == RSF_BACKEND_OK, "FG can resume");
    record(self, 6, RSF_FRAME_FLAG_RESET);
    auto& slot = self.slots[6 % self.slots.size()]; slot.completion_unknown = true;
    require(rsf_ffxgvk_present(&self, &info) == VK_SUCCESS && !self.status.enabled && dispatch_calls == 4,
        "unknown GPU completion never admits an input lease");
    self.chain = VK_NULL_HANDLE;
    for (auto& value : self.slots) { value.color.image = VK_NULL_HANDLE; value.consumer_completion = VK_NULL_HANDLE; }
    std::puts("PASS: FFX camera resets preserve presenter activation and SDK reset dispatch");
    return 0;
}
