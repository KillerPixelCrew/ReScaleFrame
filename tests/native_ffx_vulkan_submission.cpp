// SPDX-License-Identifier: GPL-3.0-only
// Exercise the actual owner with synthetic recording/submission functions.
#include "../runtime/orchestrator/src/native_fg_vulkan.cpp"
#include <cstdio>
#include <cstdlib>

namespace {
uint64_t handles = 100;
uint32_t resets = 0, submits = 0, waits = 0, copies = 0, sdk_records = 0;
bool reject_prepare = false, reject_wait = false;
void require(bool value, const char* text) { if (!value) { std::fprintf(stderr, "%s\n", text); std::exit(1); } }
template<class T> T handle() { return reinterpret_cast<T>(++handles); }
VkResult VKAPI_PTR create_fence(VkDevice, const VkFenceCreateInfo*, const VkAllocationCallbacks*, VkFence* out) { *out = handle<VkFence>(); return VK_SUCCESS; }
void VKAPI_PTR destroy_fence(VkDevice, VkFence, const VkAllocationCallbacks*) {}
VkResult VKAPI_PTR reset_fences(VkDevice, uint32_t count, const VkFence* fence) { require(count == 1 && *fence, "reset has a real completion identity"); ++resets; return VK_SUCCESS; }
VkResult VKAPI_PTR wait_fences(VkDevice, uint32_t count, const VkFence* fence, VkBool32, uint64_t timeout) {
    require(count == 1 && *fence && timeout == 15000000000ull, "wait observes the earlier completion before reset");
    ++waits; return reject_wait ? VK_TIMEOUT : VK_SUCCESS;
}
int32_t submit(void*, uint32_t count, const void* commands, uint64_t fence) {
    require(!count && !commands && fence, "fences are queued after successful host submissions"); ++submits; return VK_SUCCESS;
}
VkResult VKAPI_PTR create_image(VkDevice, const VkImageCreateInfo*, const VkAllocationCallbacks*, VkImage* out) { *out = handle<VkImage>(); return VK_SUCCESS; }
void VKAPI_PTR destroy_image(VkDevice, VkImage, const VkAllocationCallbacks*) {}
void VKAPI_PTR requirements(VkDevice, VkImage, VkMemoryRequirements* out) { out->size = 4096; out->memoryTypeBits = 1; }
VkResult VKAPI_PTR allocate(VkDevice, const VkMemoryAllocateInfo*, const VkAllocationCallbacks*, VkDeviceMemory* out) { *out = handle<VkDeviceMemory>(); return VK_SUCCESS; }
void VKAPI_PTR free_memory(VkDevice, VkDeviceMemory, const VkAllocationCallbacks*) {}
VkResult VKAPI_PTR bind(VkDevice, VkImage, VkDeviceMemory, VkDeviceSize) { return VK_SUCCESS; }
void VKAPI_PTR barrier(VkCommandBuffer, VkPipelineStageFlags, VkPipelineStageFlags, VkDependencyFlags, uint32_t, const VkMemoryBarrier*,
    uint32_t, const VkBufferMemoryBarrier*, uint32_t, const VkImageMemoryBarrier*) {}
void VKAPI_PTR copy(VkCommandBuffer, VkImage, VkImageLayout, VkImage, VkImageLayout, uint32_t, const VkImageCopy*) { ++copies; }
ffxReturnCode_t dispatch(ffxContext*, const ffxDispatchDescHeader* header) {
    require(header->type == FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE, "real prepare dispatch reached the fake SDK");
    ++sdk_records; // A failing SDK call can already have recorded GPU work.
    return reject_prepare ? FFX_API_RETURN_ERROR_PARAMETER : FFX_API_RETURN_OK;
}
ffxReturnCode_t configure(ffxContext*, const ffxConfigureDescHeader*) { return FFX_API_RETURN_OK; }
VkResult VKAPI_PTR present(VkQueue, const VkPresentInfoKHR*) { return VK_SUCCESS; }
rsf_frame_record record(uint64_t session, uint64_t frame, uint32_t view = 0, uint32_t generation = 1) {
    rsf_frame_record result{}; result.struct_size = sizeof(result); result.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    result.session_id = session; result.frame_id = frame; result.view_id = view; result.resource_generation = generation;
    result.render_width = result.output_width = 640; result.render_height = result.output_height = 360; result.frame_time_ms = 16.667f;
    auto& camera = result.camera; camera.struct_size = sizeof(camera); camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    camera.render_width = camera.output_width = 640; camera.render_height = camera.output_height = 360;
    camera.near_plane = 0.05f; camera.far_plane = 100; camera.vertical_fov_radians = 1; camera.depth_inverted = 1;
    for (auto* matrix : {camera.view_to_clip, camera.clip_to_view, camera.view_to_world, camera.world_to_view, camera.clip_to_previous_clip})
        for (uint32_t i = 0; i < 16; i += 5) matrix[i] = 1;
    return result;
}
rsf_fgvk_frame frame(rsf_ffxgvk& self, const rsf_frame_record& source) {
    rsf_fgvk_frame result{}; result.struct_size = sizeof(result); result.abi_version = RSF_FGVK_ABI_VERSION; result.record = &source;
    auto image = [&](uint32_t format) { return rsf_srvk_image{self.setup.graphics.device, 12, format, 640, 360, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1}; };
    result.depth = image(VK_FORMAT_R32_SFLOAT); result.motion = image(VK_FORMAT_R16G16_SFLOAT); result.hudless = image(VK_FORMAT_R8G8B8A8_UNORM);
    result.motion_scale_x = 640; result.motion_scale_y = 360; return result;
}
void complete(rsf_ffxgvk& self, const rsf_frame_record& source, void* prepare, void* hudless) {
    auto input = frame(self, source);
    require(rsf_ffxgvk_prepare(&self, prepare, &input) == RSF_BACKEND_OK && rsf_ffxgvk_hudless(&self, hudless, &input) == RSF_BACKEND_OK,
        "real prepare and HUD-less recording accepted the source");
}
}
extern "C" int32_t rsf_slvk_without_dlssg(int32_t (*call)(void*), void* user) { return call(user); }
extern "C" int32_t rsf_latencyvk_wsi_call(rsf_vk_wsi_call call, void* user) { return call(user); }
extern "C" int32_t rsf_latencyvk_queue_present(rsf_vk_function call, void* queue, const void* info) {
    return reinterpret_cast<PFN_vkQueuePresentKHR>(call)(static_cast<VkQueue>(queue), static_cast<const VkPresentInfoKHR*>(info));
}
int main() {
    SetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", nullptr); SetEnvironmentVariableW(L"RSF_VULKAN_TIMING_FILE", nullptr);
    rsf_ffxgvk self{}; self.setup.graphics.device = reinterpret_cast<void*>(1); self.setup.graphics.depth_inverted = 1;
    self.setup.graphics_submit = submit; self.setup.queues[0] = reinterpret_cast<void*>(2); self.width = 640; self.height = 360; self.enabled = true;
    self.create_fence = create_fence; self.destroy_fence = destroy_fence; self.reset_fences = reset_fences; self.wait_fences = wait_fences;
    self.create_image = create_image; self.destroy_image = destroy_image; self.requirements = requirements; self.allocate = allocate;
    self.free_memory = free_memory; self.bind = bind; self.barrier = barrier; self.copy = copy; self.dispatch = dispatch; self.configure = configure;
    self.memory_properties.memoryTypeCount = 1; self.memory_properties.memoryTypes[0].propertyFlags = VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
    self.chain = reinterpret_cast<VkSwapchainKHR>(3); self.functions.pOutQueuePresentKHR = present;
    VkPresentInfoKHR info{}; info.sType = VK_STRUCTURE_TYPE_PRESENT_INFO_KHR; info.swapchainCount = 1; info.pSwapchains = &self.chain;
    void* a = reinterpret_cast<void*>(10); void* b = reinterpret_cast<void*>(11); void* c = reinterpret_cast<void*>(12);
    auto one = record(7, 1); complete(self, one, a, a);
    void* duplicate[] = {a, a, nullptr}; rsf_ffxgvk_submitted(&self, duplicate, 3);
    require(submits == 1 && resets == 1 && self.slots[1].submitted_prepare && self.slots[1].submitted_hudless,
        "one command buffer with both roles gets exactly one completion fence");
    self.slots[1].source_pending = false; // The prior fence was observed complete, without replacing that source.
    rsf_ffxgvk_submitted(&self, duplicate, 3);
    require(submits == 1 && resets == 1, "repeated notification never rearms an accepted old slot");
    auto two = record(7, 2); auto input = frame(self, two);
    require(rsf_ffxgvk_prepare(&self, a, &input) == RSF_BACKEND_OK, "reused command address records another source");
    rsf_ffxgvk_submitted(&self, &a, 1);
    require(submits == 2 && self.slots[2].source_pending && !self.slots[1].source_pending,
        "prepare-only source is fenced without rearming the older accepted source");
    auto three = record(7, 3); complete(self, three, b, b);
    auto four = record(7, 4, 2); complete(self, four, b, b);
    rsf_ffxgvk_submitted(&self, &b, 1);
    require(submits == 3 && !self.slots[3].recorded_prepare && !self.slots[3].recorded_hudless && !self.slots[3].prepared &&
        !self.slots[3].color.initialized && self.slots[4].submitted_prepare && self.slots[4].submitted_hudless,
        "newest pending recording wins and abandoned source/first-use state is invalidated");
    require(rsf_ffxgvk_present(&self, &info) == VK_SUCCESS && self.status.enabled, "newest submitted source is presentation eligible");
    auto five = record(7, 5); input = frame(self, five); reject_prepare = true;
    require(rsf_ffxgvk_prepare(&self, c, &input) == RSF_BACKEND_ERROR_FEATURE_FAILED && self.slots[5].recorded_prepare && !self.slots[5].prepared,
        "SDK recording failure remains distinct from accepted FG preparation"); reject_prepare = false;
    auto before = submits; rsf_ffxgvk_submitted(&self, &c, 1);
    require(submits == before + 1 && self.slots[5].source_pending, "partially recorded SDK failure still gets submission completion");
    require(rsf_ffxgvk_present(&self, &info) == VK_SUCCESS && !self.status.enabled, "failed prepare does not replay the previous source");
    auto six = record(7, 6); input = frame(self, six);
    require(rsf_ffxgvk_prepare(&self, a, &input) == RSF_BACKEND_OK, "split prepare accepted");
    rsf_ffxgvk_submitted(&self, &a, 1); before = submits; const auto earlier_waits = waits;
    require(rsf_ffxgvk_hudless(&self, b, &input) == RSF_BACKEND_OK, "split HUD-less accepted"); rsf_ffxgvk_submitted(&self, &b, 1);
    require(submits == before + 1 && waits == earlier_waits + 1, "later role waits prior fence before establishing a new completion point");
    rsf_ffxgvk_submitted(&self, &b, 1); require(submits == before + 1, "split duplicate submission is idempotent");
    require(rsf_ffxgvk_present(&self, &info) == VK_SUCCESS && self.status.enabled, "split submitted roles can generate");
    auto seven = record(7, 7); input = frame(self, seven);
    require(rsf_ffxgvk_prepare(&self, a, &input) == RSF_BACKEND_OK, "completed ring slot can be reused"); rsf_ffxgvk_submitted(&self, &a, 1);
    require(rsf_ffxgvk_hudless(&self, b, &input) == RSF_BACKEND_OK, "later role recorded"); before = resets; reject_wait = true;
    rsf_ffxgvk_submitted(&self, &b, 1); reject_wait = false;
    require(self.slots[1].completion_unknown && resets == before, "positive fence timeout preserves unknown completion without reset");
    require(rsf_ffxgvk_present(&self, &info) == VK_SUCCESS && !self.status.enabled, "unknown completion refuses FG");
    auto new_session = record(99, 2, 8, 3); input = frame(self, new_session);
    require(rsf_ffxgvk_prepare(&self, c, &input) == RSF_BACKEND_OK, "new session can restart original source IDs");
    for (uint32_t kind = 0; kind < 3; ++kind) {
        auto foreign = new_session; if (kind == 0) ++foreign.session_id; if (kind == 1) ++foreign.view_id; if (kind == 2) ++foreign.resource_generation;
        auto wrong = frame(self, foreign); const auto previous_copies = copies;
        require(rsf_ffxgvk_hudless(&self, c, &wrong) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && copies == previous_copies,
            "same numeric frame never joins another session, view or resource generation");
    }
    require(rsf_ffxgvk_hudless(&self, c, &input) == RSF_BACKEND_OK, "matching identity completes new session"); rsf_ffxgvk_submitted(&self, &c, 1);
    require(rsf_ffxgvk_present(&self, &info) == VK_SUCCESS && self.status.enabled && self.status.source_frame == 2,
        "presentation admission uses the local recording order, not old session frame IDs");
    require(rsf_ffxgvk_present(&self, &info) == VK_SUCCESS && !self.status.enabled, "same recording cannot present twice");
    require(sdk_records >= 8, "the scenarios exercised real SDK prepare callsites");
    self.chain = VK_NULL_HANDLE;
    std::puts("PASS: FFX source recording, reused command identity and submission completion");
}
