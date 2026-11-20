// SPDX-License-Identifier: MIT
// Compiles the enabled SDK/Vulkan leaves. All callbacks are synthetic.
#include "../runtime/orchestrator/src/native_fg_dlss_vulkan.cpp"
#include "../runtime/orchestrator/src/native_fg_dlss_vulkan_capture.cpp"
#include <cstdio>
#include <cstdlib>

namespace {
uint64_t lease_session = 7, lease_frame = 42;
uint32_t sdk_token = 42;
uint32_t on_calls = 0, off_calls = 0, present_calls = 0, fence_submits = 0, destroyed_chains = 0;
uint32_t sdk_presented = 0, copies = 0, release_calls = 0, max_generated = 3;
bool refuse_create = false, refuse_enumeration = false;
bool refuse_options = false, refuse_submit = false, refuse_tags = false, refuse_release = false;
uint64_t worker_timeout_epoch = 0;
bool worker_timeout = false, locked = false, refuse_inputs_wait = false;
uint32_t inputs_waits = 0;
uint64_t expected_inputs_value = 9, completion_value = 9;
const auto device = reinterpret_cast<VkDevice>(1);
const auto queue = reinterpret_cast<VkQueue>(2);
const auto chain = reinterpret_cast<VkSwapchainKHR>(3);
const auto* allocator = reinterpret_cast<const VkAllocationCallbacks*>(4);
auto* prepare_list = reinterpret_cast<void*>(10);
auto* color_list = reinterpret_cast<void*>(11);
uint64_t next_handle = 1000;

void require(bool condition, const char* message) {
    if (!condition) { std::fprintf(stderr, "%s\n", message); std::exit(1); }
}

sl::Result options(const sl::ViewportHandle& view, const sl::DLSSGOptions& value) {
    require(static_cast<uint32_t>(view) == 0 && value.numBackBuffers == 3, "FG uses root viewport zero and enumerated image count");
    require(value.queueParallelismMode == sl::DLSSGQueueParallelismMode::eBlockNoClientQueues,
        "SDK never queues a future timeline wait ahead of client binary waits");
    require(value.enableUserInterfaceRecomposition == sl::Boolean::eFalse, "missing UI producer does not enable recomposition");
    require((static_cast<uint32_t>(value.flags) & static_cast<uint32_t>(sl::DLSSGFlags::eRetainResourcesWhenOff)) != 0,
        "Off keeps resources available for checked retirement");
    if (refuse_options) return sl::Result::eErrorInvalidParameter;
    if (value.mode == sl::DLSSGMode::eOn) {
        ++on_calls;
        require(value.numFramesToGenerate == 3, "MFG forwards generated count excluding real frame");
    } else {
        ++off_calls;
        require(value.numFramesToGenerate == 1, "Off publishes the SDK baseline count");
    }
    return sl::Result::eOk;
}
sl::Result state(const sl::ViewportHandle&, sl::DLSSGState& value, const sl::DLSSGOptions*) {
    value.numFramesToGenerateMax = max_generated; value.minWidthOrHeight = 128;
    value.numFramesActuallyPresented = sdk_presented; sdk_presented = 0;
    value.inputsProcessingCompletionFence = reinterpret_cast<void*>(0xc9);
    value.lastPresentInputsProcessingCompletionFenceValue = completion_value;
    return sl::Result::eOk;
}
sl::Result set_constants(const sl::Constants& data, const sl::FrameToken& token, const sl::ViewportHandle&) {
    require(&token == reinterpret_cast<const sl::FrameToken*>(&sdk_token), "constants borrow the original SDK token");
    require(data.mvecScale.x == 0.5f && data.mvecScale.y == -0.5f, "motion scale uses the render dimensions");
    return sl::Result::eOk;
}
sl::Result set_tags(const sl::FrameToken& token, const sl::ViewportHandle&, const sl::ResourceTag* tags, uint32_t count, sl::CommandBuffer* commands) {
    require(&token == reinterpret_cast<const sl::FrameToken*>(&sdk_token) && count == 2, "tags borrow the original SDK token");
    require(tags[0].lifecycle == sl::ResourceLifecycle::eOnlyValidNow && tags[0].resource &&
        reinterpret_cast<uint64_t>(tags[0].resource->native) >= 1000, "tag uses an owned Vulkan image, not the engine image");
    if (commands == reinterpret_cast<sl::CommandBuffer*>(prepare_list)) {
        require(tags[0].type == sl::kBufferTypeDepth && tags[1].type == sl::kBufferTypeMotionVectors, "prepare uses typed guide tags");
    } else {
        require(commands == reinterpret_cast<sl::CommandBuffer*>(color_list) && tags[0].type == sl::kBufferTypeHUDLessColor &&
            tags[1].type == sl::kBufferTypeUIColorAndAlpha && !tags[1].resource, "HUD-less explicitly clears absent UI recomposition input");
    }
    return refuse_tags ? sl::Result::eErrorInvalidParameter : sl::Result::eOk;
}
sl::Result free_resources(sl::Feature feature, const sl::ViewportHandle& view) {
    require(feature == sl::kFeatureDLSS_G && static_cast<uint32_t>(view) == 0, "release addresses the shared viewport");
    ++release_calls;
    return refuse_release ? sl::Result::eErrorInvalidParameter : sl::Result::eOk;
}
rsf_backend_result fake_initialize(void*, uintptr_t, void* user) {
    const auto& call = *static_cast<rsf_dlgvk_sdk_call*>(user);
    auto& self = *static_cast<Native*>(call.native);
    self.constants = set_constants; self.tags = set_tags; self.release = free_resources;
    self.options = options; self.state = state;
    return RSF_BACKEND_OK;
}

VkResult VKAPI_PTR create_images(VkDevice value, const VkSwapchainCreateInfoKHR* info, const VkAllocationCallbacks* used, VkSwapchainKHR* out) {
    require(value == device && used == allocator && !locked && (info->imageUsage & VK_IMAGE_USAGE_TRANSFER_DST_BIT),
        "native proxy creation retains the allocator and adds WSI destination usage outside the broker");
    *out = chain; return refuse_create ? VK_ERROR_INITIALIZATION_FAILED : VK_SUCCESS;
}
void VKAPI_PTR destroy_images(VkDevice value, VkSwapchainKHR actual, const VkAllocationCallbacks* used) {
    require(value == device && actual == chain && used == allocator && !locked, "native proxy destroy uses exact device, chain and allocator");
    ++destroyed_chains;
}
VkResult VKAPI_PTR enumerate_images(VkDevice value, VkSwapchainKHR actual, uint32_t* count, VkImage* images) {
    require(value == device && actual == chain, "enumeration uses the owned proxy");
    if (refuse_enumeration) return VK_ERROR_INITIALIZATION_FAILED;
    if (!images) { *count = 3; return VK_SUCCESS; }
    if (*count > 3) { *count = 3; return VK_INCOMPLETE; }
    for (uint32_t i = 0; i < *count; ++i) images[i] = reinterpret_cast<VkImage>(uintptr_t(100 + i));
    return *count < 3 ? VK_INCOMPLETE : VK_SUCCESS;
}
VkResult VKAPI_PTR acquire_image(VkDevice value, VkSwapchainKHR actual, uint64_t timeout, VkSemaphore semaphore, VkFence fence, uint32_t* index) {
    require(value == device && actual == chain && timeout == 8 && semaphore == reinterpret_cast<VkSemaphore>(9) &&
        fence == reinterpret_cast<VkFence>(10), "acquire preserves exact application synchronization arguments");
    *index = 2; return VK_SUBOPTIMAL_KHR;
}
VkResult VKAPI_PTR publish(VkQueue actual, const VkPresentInfoKHR* info) {
    require(actual == queue && info->pSwapchains[0] == chain && !locked, "proxy presents use the exact graphics queue outside its WSI broker");
    ++present_calls; return VK_SUCCESS;
}
VkResult VKAPI_PTR idle(VkDevice value) {
    require(value == device && locked, "proxy device idle uses the queue broker");
    if (worker_timeout) ++worker_timeout_epoch;
    return VK_SUCCESS;
}
VkResult VKAPI_PTR wait_semaphores(VkDevice, const VkSemaphoreWaitInfo* info, uint64_t timeout) {
    require(info->semaphoreCount == 1 && info->pSemaphores[0] == reinterpret_cast<VkSemaphore>(0xc9) &&
        info->pValues[0] == expected_inputs_value && timeout == 15000000000ull, "waits the captured SDK completion value, not a source frame ID");
    ++inputs_waits; return refuse_inputs_wait ? VK_TIMEOUT : VK_SUCCESS;
}
void lock(void*) { require(!locked, "broker lock is never nested"); locked = true; }
void unlock(void*) { require(locked, "broker unlock follows lock"); locked = false; }
int32_t submit(void* user, uint32_t count, const void* commands, uint64_t fence) {
    require(user == queue && count == 0 && !commands && fence, "completion fences follow real host graphics submissions");
    ++fence_submits; return refuse_submit ? VK_ERROR_DEVICE_LOST : VK_SUCCESS;
}
VkResult VKAPI_PTR create_fence(VkDevice, const VkFenceCreateInfo*, const VkAllocationCallbacks*, VkFence* fence) {
    *fence = reinterpret_cast<VkFence>(++next_handle); return VK_SUCCESS;
}
VkResult VKAPI_PTR reset_fences(VkDevice, uint32_t, const VkFence*) { return VK_SUCCESS; }
VkResult VKAPI_PTR wait_fences(VkDevice, uint32_t, const VkFence*, VkBool32, uint64_t) { return VK_SUCCESS; }
void VKAPI_PTR destroy_fence(VkDevice, VkFence, const VkAllocationCallbacks*) {}
VkResult VKAPI_PTR create_image(VkDevice, const VkImageCreateInfo*, const VkAllocationCallbacks*, VkImage* image) {
    *image = reinterpret_cast<VkImage>(++next_handle); return VK_SUCCESS;
}
void VKAPI_PTR destroy_image(VkDevice, VkImage, const VkAllocationCallbacks*) {}
void VKAPI_PTR image_requirements(VkDevice, VkImage, VkMemoryRequirements* info) { *info = {4096, 8, 1}; }
VkResult VKAPI_PTR allocate(VkDevice, const VkMemoryAllocateInfo*, const VkAllocationCallbacks*, VkDeviceMemory* memory) {
    *memory = reinterpret_cast<VkDeviceMemory>(++next_handle); return VK_SUCCESS;
}
void VKAPI_PTR free_memory(VkDevice, VkDeviceMemory, const VkAllocationCallbacks*) {}
VkResult VKAPI_PTR bind_image(VkDevice, VkImage, VkDeviceMemory, VkDeviceSize) { return VK_SUCCESS; }
VkResult VKAPI_PTR create_view(VkDevice, const VkImageViewCreateInfo*, const VkAllocationCallbacks*, VkImageView* view) {
    *view = reinterpret_cast<VkImageView>(++next_handle); return VK_SUCCESS;
}
void VKAPI_PTR destroy_view(VkDevice, VkImageView, const VkAllocationCallbacks*) {}
void VKAPI_PTR barrier(VkCommandBuffer, VkPipelineStageFlags, VkPipelineStageFlags, VkDependencyFlags, uint32_t,
    const VkMemoryBarrier*, uint32_t, const VkBufferMemoryBarrier*, uint32_t, const VkImageMemoryBarrier*) {}
void VKAPI_PTR copy_image(VkCommandBuffer, VkImage source, VkImageLayout, VkImage destination, VkImageLayout, uint32_t, const VkImageCopy*) {
    require(reinterpret_cast<uint64_t>(source) < 1000 && reinterpret_cast<uint64_t>(destination) >= 1000,
        "private image copies read the leased engine image");
    ++copies;
}
void VKAPI_PTR memory_properties(VkPhysicalDevice, VkPhysicalDeviceMemoryProperties* info) {
    *info = {}; info->memoryTypeCount = 1; info->memoryTypes[0].propertyFlags = VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
}
VkResult VKAPI_PTR surface_capabilities(VkPhysicalDevice, VkSurfaceKHR, VkSurfaceCapabilitiesKHR* info) {
    *info = {}; info->supportedUsageFlags = VK_IMAGE_USAGE_TRANSFER_DST_BIT; return VK_SUCCESS;
}
rsf_vk_function device_proc(void*, const char* name) {
#define MOCK(vk, fn) if (std::strcmp(name, #vk) == 0) return reinterpret_cast<rsf_vk_function>(fn)
    MOCK(vkCreateSwapchainKHR, create_images); MOCK(vkDestroySwapchainKHR, destroy_images);
    MOCK(vkGetSwapchainImagesKHR, enumerate_images); MOCK(vkAcquireNextImageKHR, acquire_image);
    MOCK(vkQueuePresentKHR, publish); MOCK(vkDeviceWaitIdle, idle); MOCK(vkWaitSemaphores, wait_semaphores);
    MOCK(vkCreateFence, create_fence); MOCK(vkDestroyFence, destroy_fence); MOCK(vkResetFences, reset_fences); MOCK(vkWaitForFences, wait_fences);
    MOCK(vkCreateImage, create_image); MOCK(vkDestroyImage, destroy_image); MOCK(vkGetImageMemoryRequirements, image_requirements);
    MOCK(vkAllocateMemory, allocate); MOCK(vkFreeMemory, free_memory); MOCK(vkBindImageMemory, bind_image);
    MOCK(vkCreateImageView, create_view); MOCK(vkDestroyImageView, destroy_view); MOCK(vkCmdPipelineBarrier, barrier); MOCK(vkCmdCopyImage, copy_image);
    MOCK(vkGetPhysicalDeviceMemoryProperties, memory_properties); MOCK(vkGetPhysicalDeviceSurfaceCapabilitiesKHR, surface_capabilities);
#undef MOCK
    return nullptr;
}

struct Source {
    rsf_frame_record record{};
    rsf_fgvk_frame input{};
    explicit Source(uint64_t frame) {
        record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.session_id = lease_session; record.frame_id = lease_frame = frame;
        record.render_width = 640; record.render_height = 360; record.output_width = 1280; record.output_height = 720;
        record.frame_time_ms = 16.0f;
        auto& c = record.camera;
        c.struct_size = sizeof(c); c.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
        c.render_width = record.render_width; c.render_height = record.render_height;
        c.output_width = record.output_width; c.output_height = record.output_height;
        c.near_plane = 0.1f; c.far_plane = 500.0f; c.vertical_fov_radians = 1.0f;
        for (uint32_t i = 0; i < 16; i += 5) c.clip_to_previous_clip[i] = 1.0f;
        input.struct_size = sizeof(input); input.abi_version = RSF_FGVK_ABI_VERSION; input.record = &record;
        input.depth = {device, 99, VK_FORMAT_R32_SFLOAT, 640, 360, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
        input.motion = {device, 98, VK_FORMAT_R16G16_SFLOAT, 640, 360, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
        input.hudless = {device, 97, VK_FORMAT_R8G8B8A8_UNORM, 1280, 720, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
        input.motion_scale_x = 320.0f; input.motion_scale_y = -180.0f;
    }
    void stage(rsf_dlssgvk* owner) const {
        require(rsf_dlssgvk_prepare(owner, prepare_list, &input) == RSF_BACKEND_OK &&
            rsf_dlssgvk_hudless(owner, color_list, &input) == RSF_BACKEND_OK, "source stages and tags the guides and HUD-less image");
    }
};
rsf_dlssgvk_status status(rsf_dlssgvk* owner) {
    rsf_dlssgvk_status result{sizeof(result), RSF_DLSSGVK_ABI_VERSION};
    require(rsf_dlssgvk_get_status(owner, &result) == RSF_BACKEND_OK, "status snapshot is available");
    return result;
}
}
extern "C" rsf_backend_result rsf_slvk_invoke(const rsf_slvk_call* call) {
    require(!locked && call->struct_size == sizeof(*call) && call->abi_version == RSF_SLVK_ABI_VERSION,
        "worker receives versioned call outside the host queue lock");
    if (call->session_id && (call->session_id != lease_session || call->frame_id != lease_frame)) return RSF_BACKEND_ERROR_NOT_READY;
    return call->callback(reinterpret_cast<void*>(1), call->frame_id ? reinterpret_cast<uintptr_t>(&sdk_token) : 0, call->user);
}
extern "C" uint64_t VulkanStorySlWorkerTimeoutEpoch() { return worker_timeout_epoch; }
int main() {
    SetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", nullptr);
    require(rsf_dlssgvk_create(nullptr, nullptr, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "null create refused");
    require(rsf_dlssgvk_prepare(nullptr, nullptr, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "null recording refused");
    require(rsf_dlssgvk_destroy(nullptr) == RSF_BACKEND_OK, "null destruction idempotent");

    static const auto synthetic = [] { auto result = platform; result.initialize = fake_initialize; return result; }();
    VkSwapchainCreateInfoKHR create{VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR};
    create.surface = reinterpret_cast<VkSurfaceKHR>(5); create.imageExtent = {1280, 720};
    create.imageFormat = VK_FORMAT_B8G8R8A8_UNORM; create.imageArrayLayers = 1;
    rsf_fgvk_setup setup{sizeof(setup), RSF_FGVK_ABI_VERSION};
    setup.graphics = {sizeof(setup.graphics), RSF_SRVK_ABI_VERSION};
    setup.graphics.instance = reinterpret_cast<void*>(6); setup.graphics.physical_device = reinterpret_cast<void*>(7);
    setup.graphics.device = device; setup.graphics.instance_proc = setup.graphics.device_proc = device_proc;
    setup.graphics.output_width = 1280; setup.graphics.output_height = 720;
    setup.queues[0] = queue; setup.queue_user = queue; setup.create_info = &create; setup.allocator = allocator;
    setup.graphics_submit = submit; setup.lock_queues = lock; setup.unlock_queues = unlock;
    rsf_dlssgvk* owner{}; uint64_t chain_out{};
    require(rsf_core_dlgvk_create(&synthetic, &setup, &owner, &chain_out) == RSF_BACKEND_OK && owner &&
        chain_out == reinterpret_cast<uint64_t>(chain), "Rust publishes only the complete proxy owner");
    rsf_dlssgvk* second{}; uint64_t second_chain{};
    require(rsf_core_dlgvk_create(&synthetic, &setup, &second, &second_chain) == RSF_BACKEND_ERROR_NOT_READY &&
        !second && !second_chain, "another owner cannot claim the one SDK viewport");
    uint64_t images[8]{}; uint32_t count = 8;
    require(rsf_dlssgvk_images(owner, &count, images) == VK_SUCCESS && count == 3 && images[2] == 102,
        "proxy image enumeration normalizes oversized capacities");
    count = 2;
    require(rsf_dlssgvk_images(owner, &count, images) == VK_INCOMPLETE && count == 2, "short proxy enumeration remains incomplete");
    uint32_t index = 0, layout = 0;
    require(rsf_dlssgvk_acquire(owner, 8, 9, 10, &index) == VK_SUBOPTIMAL_KHR && index == 2, "acquire preserves native result");
    require(rsf_dlssgvk_image_layout(owner, &layout) == RSF_BACKEND_OK && layout == VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL &&
        rsf_dlssgvk_image_layout(owner, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT,
        "game-facing proxy images require transfer source layout even while Off");
    require(rsf_dlssgvk_enable(owner, 1, 4) == RSF_BACKEND_ERROR_NOT_SUPPORTED &&
        rsf_dlssgvk_enable(owner, 1, 0) == RSF_BACKEND_ERROR_INVALID_ARGUMENT &&
        rsf_dlssgvk_enable(owner, 1, 3) == RSF_BACKEND_OK, "MFG count is validated without clamping");

    VkPresentInfoKHR info{VK_STRUCTURE_TYPE_PRESENT_INFO_KHR};
    info.swapchainCount = 1; info.pSwapchains = &chain;
    void* lists[]{prepare_list, color_list};
    Source first(42); first.stage(owner);
    require(rsf_dlssgvk_present(owner, &info) == VK_SUCCESS && !on_calls && off_calls == 1 && !status(owner).presentation.enabled,
        "unsubmitted source explicitly publishes Off");
    Source second_source(43); second_source.stage(owner);
    rsf_dlssgvk_submitted(owner, lists, 1);
    require(fence_submits == 1 && rsf_dlssgvk_present(owner, &info) == VK_SUCCESS && off_calls == 2,
        "prepare-only submission is fenced but still explicitly Off");
    Source complete(44); complete.stage(owner);
    rsf_dlssgvk_submitted(owner, lists, 2);
    const auto previous_submits = fence_submits;
    rsf_dlssgvk_submitted(owner, lists, 2);
    require(fence_submits == previous_submits, "duplicate submission does not reset a live fence");
    sdk_presented = 4;
    require(rsf_dlssgvk_present(owner, &info) == VK_SUCCESS && on_calls == 1, "fully submitted source enables MFG");
    auto snapshot = status(owner);
    require(snapshot.presentation.enabled && snapshot.presentation.source_frame == 44 &&
        snapshot.presentation.last_present_count == 4 && !snapshot.presentation.generated_callbacks,
        "actual SDK count stays distinct from source identity and generated callbacks");
    require(rsf_dlssgvk_present(owner, &info) == VK_SUCCESS && !status(owner).presentation.enabled,
        "duplicate source preserves ordinary Present");

    Source recycle(50);
    refuse_inputs_wait = true;
    const auto previous_copies = copies;
    require(rsf_dlssgvk_prepare(owner, prepare_list, &recycle.input) == RSF_BACKEND_ERROR_NOT_READY && copies == previous_copies,
        "SDK timeline timeout retains the slot and its private images");
    refuse_inputs_wait = false;
    recycle.stage(owner);
    require(inputs_waits == 2, "successful exact timeline wait permits reuse");
    rsf_dlssgvk_submitted(owner, lists, 2);
    completion_value = 0;
    require(rsf_dlssgvk_present(owner, &info) == VK_SUCCESS && on_calls == 2, "unchanged On options are published again");
    expected_inputs_value = 0;
    Source zero(56); zero.stage(owner);
    require(inputs_waits == 3, "zero is a valid captured SDK timeline value");
    rsf_dlssgvk_submitted(owner, lists, 2);
    completion_value = expected_inputs_value = 9;
    lease_frame = 57;
    require(rsf_dlssgvk_present(owner, &info) == VK_SUCCESS && on_calls == 2 && !status(owner).presentation.enabled,
        "expired original token is never replaced by a newer CPU token");

    Source unknown(57); unknown.stage(owner); refuse_submit = true;
    rsf_dlssgvk_submitted(owner, lists, 2); refuse_submit = false;
    require(rsf_dlssgvk_present(owner, &info) == VK_SUCCESS && on_calls == 2, "unknown fence completion refuses interpolation");
    lease_session = 8;
    Source old_output(1); old_output.stage(owner); rsf_dlssgvk_submitted(owner, lists, 2);
    Source new_output(2); new_output.stage(owner); rsf_dlssgvk_submitted(owner, lists, 2);
    lease_frame = 1;
    require(rsf_dlssgvk_present_source(owner, &info, &old_output.record) == VK_SUCCESS &&
        status(owner).presentation.enabled && status(owner).presentation.source_frame == 1,
        "explicit old output selects its original token even after a new session restarts source numbering");
    lease_frame = 2;
    require(rsf_dlssgvk_present_source(owner, &info, nullptr) == VK_SUCCESS && !status(owner).presentation.source_frame,
        "unidentified explicit output cannot consume the latest record");
    require(rsf_dlssgvk_present_source(owner, &info, &new_output.record) == VK_SUCCESS &&
        status(owner).presentation.enabled && status(owner).presentation.source_frame == 2, "later output consumes its exact source");
    require(rsf_dlssgvk_present_source(owner, &info, &old_output.record) == VK_SUCCESS && !status(owner).presentation.enabled &&
        rsf_dlssgvk_present_source(owner, &info, &new_output.record) == VK_SUCCESS && !status(owner).presentation.enabled,
        "an older output cannot rewind the newest consumed source");
    Source options_failure(4); options_failure.stage(owner); rsf_dlssgvk_submitted(owner, lists, 2);
    refuse_options = true;
    const auto before_refused_present = present_calls;
    require(rsf_dlssgvk_present(owner, &info) == VK_ERROR_INITIALIZATION_FAILED && present_calls == before_refused_present &&
        !status(owner).presentation.enabled, "failed enable and failed explicit Off refuse WSI");
    refuse_options = false;
    VkSwapchainKHR foreign = reinterpret_cast<VkSwapchainKHR>(99); info.pSwapchains = &foreign;
    require(rsf_dlssgvk_present(owner, &info) == VK_ERROR_INITIALIZATION_FAILED && present_calls == before_refused_present,
        "foreign chain never reaches owner WSI");
    info.pSwapchains = &chain;
    snapshot.abi_version = 99;
    require(rsf_dlssgvk_get_status(owner, &snapshot) == RSF_BACKEND_ERROR_ABI_MISMATCH &&
        status(owner).requested_generated_frames == 3, "status validates its ABI and preserves independent requested count");

    worker_timeout = true;
    require(rsf_dlssgvk_destroy(owner) == RSF_BACKEND_ERROR_NOT_READY && !destroyed_chains && !release_calls,
        "worker timeout refuses teardown even when Vulkan idle succeeds");
    worker_timeout = false; refuse_inputs_wait = true;
    require(rsf_dlssgvk_destroy(owner) == RSF_BACKEND_ERROR_NOT_READY && !destroyed_chains && !release_calls,
        "GPU idle alone cannot discard a pending SDK completion lease");
    refuse_inputs_wait = false;
    require(rsf_dlssgvk_drain(owner) == RSF_BACKEND_OK && status(owner).requested_generated_frames == 3,
        "later successful drain preserves the interpolation request");
    Source rejected(5); refuse_tags = true;
    require(rsf_dlssgvk_prepare(owner, prepare_list, &rejected.input) == RSF_BACKEND_ERROR_FEATURE_FAILED,
        "tag failure after actual staging is returned to the host");
    refuse_tags = false;
    const auto before_pending_off = off_calls;
    require(rsf_dlssgvk_destroy(owner) == RSF_BACKEND_ERROR_NOT_READY && !destroyed_chains && off_calls == before_pending_off,
        "unsubmitted copies from rejected tags preserve ownership before any disable or release");
    const auto before_rejected = fence_submits;
    rsf_dlssgvk_submitted(owner, lists, 1);
    require(fence_submits == before_rejected + 1 && rsf_dlssgvk_drain(owner) == RSF_BACKEND_OK,
        "actual submission fences rejected copies and permits retirement");
    refuse_release = true;
    require(rsf_dlssgvk_destroy(owner) == RSF_BACKEND_ERROR_FEATURE_FAILED && release_calls == 1 && !destroyed_chains,
        "failed SDK release preserves the native chain for retry");
    refuse_release = false;
    require(rsf_dlssgvk_destroy(owner) == RSF_BACKEND_OK && release_calls == 2 && destroyed_chains == 1,
        "successful retry releases the feature and destroys its chain once");
    require(rsf_core_dlgvk_create(&synthetic, &setup, &owner, &chain_out) == RSF_BACKEND_OK &&
        rsf_dlssgvk_present(owner, &info) == VK_SUCCESS && !status(owner).presentation.enabled &&
        rsf_dlssgvk_destroy(owner) == RSF_BACKEND_OK && destroyed_chains == 2, "a new owner explicitly configures Off and retires cleanly");
    max_generated = 0;
    require(rsf_core_dlgvk_create(&synthetic, &setup, &owner, &chain_out) == RSF_BACKEND_ERROR_NOT_SUPPORTED &&
        !owner && !chain_out && destroyed_chains == 2, "zero supported generation count refuses before proxy creation");
    max_generated = 3;
    refuse_create = true;
    require(rsf_core_dlgvk_create(&synthetic, &setup, &owner, &chain_out) == RSF_BACKEND_ERROR_INIT_FAILED &&
        !owner && !chain_out && destroyed_chains == 2, "a failed create result never grants ownership of its output handle");
    refuse_create = false; refuse_enumeration = true;
    require(rsf_core_dlgvk_create(&synthetic, &setup, &owner, &chain_out) == RSF_BACKEND_ERROR_INIT_FAILED &&
        !owner && !chain_out && destroyed_chains == 3, "enumeration refusal closes the unpublished proxy");
    std::puts("Synthetic DLSS Vulkan FG identity, MFG and WSI checks passed");
}
