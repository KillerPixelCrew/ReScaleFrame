// SPDX-License-Identifier: MIT
// Compiles the enabled SDK/Vulkan leaves. All callbacks are synthetic.
#include "../runtime/orchestrator/src/native_fg_dlss_vulkan.cpp"
#include "../runtime/orchestrator/src/native_fg_dlss_vulkan_capture.cpp"
#include <cstdio>
#include <cstdlib>
#include "vulkan-fg/dlss_abi.h"

namespace {
rsf_dlss_fixture_state fixture{
    7, 42,
    0, 0, 0, 0, 0,
    0, 0, 0, 3, 3,
    VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT,
    VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT, 0, 0,
    false, false, false, false, false, false, false, false,
    9, 9,
};
auto& lease_session = fixture.lease_session;
auto& lease_frame = fixture.lease_frame;
auto& on_calls = fixture.on_calls;
auto& off_calls = fixture.off_calls;
auto& present_calls = fixture.present_calls;
auto& fence_submits = fixture.fence_submits;
auto& destroyed_chains = fixture.destroyed_chains;
auto& sdk_presented = fixture.sdk_presented;
auto& copies = fixture.copies;
auto& release_calls = fixture.release_calls;
auto& max_generated = fixture.max_generated;
auto& backbuffer_count = fixture.backbuffer_count;
auto& surface_usage = fixture.surface_usage;
auto& expected_chain_usage = fixture.expected_chain_usage;
auto& chain_creates = fixture.chain_creates;
auto& inputs_waits = fixture.inputs_waits;
auto& refuse_create = fixture.refuse_create;
auto& refuse_enumeration = fixture.refuse_enumeration;
auto& refuse_options = fixture.refuse_options;
auto& refuse_submit = fixture.refuse_submit;
auto& refuse_tags = fixture.refuse_tags;
auto& refuse_release = fixture.refuse_release;
auto& worker_timeout = fixture.worker_timeout;
auto& refuse_inputs_wait = fixture.refuse_inputs_wait;
auto& expected_inputs_value = fixture.expected_inputs_value;
auto& completion_value = fixture.completion_value;
uint32_t sdk_token = 42;
uint64_t worker_timeout_epoch = 0;
bool locked = false;
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
    require(value.structVersion == sl::kStructVersion5 && value.bReserved15 == sl::Boolean::eInvalid &&
        value.dynamicTargetFrameRate == 0.0f && value.onErrorCallback == on_error,
        "packed options keep the pinned defaults and native API-error callback");
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
    require(data.structVersion == sl::kStructVersion2 && data.cameraAspectRatio == 1280.0f / 720.0f &&
        data.clipToLensClip[0].x == sl::INVALID_FLOAT && data.cameraMotionIncluded == sl::Boolean::eTrue,
        "packed camera keeps SDK defaults and the presented aspect ratio");
    require(data.prevClipToClip[0].x == 1.0f && data.prevClipToClip[3].w == 1.0f,
        "native inverse fills the packed camera before dispatch");
    return sl::Result::eOk;
}
sl::Result set_tags(const sl::FrameToken& token, const sl::ViewportHandle&, const sl::ResourceTag* tags, uint32_t count, sl::CommandBuffer* commands) {
    require(&token == reinterpret_cast<const sl::FrameToken*>(&sdk_token) && count == 2, "tags borrow the original SDK token");
    require(tags[0].lifecycle == sl::ResourceLifecycle::eOnlyValidNow && tags[0].resource &&
        reinterpret_cast<uint64_t>(tags[0].resource->native) >= 1000, "tag uses an owned Vulkan image, not the engine image");
    if (commands == reinterpret_cast<sl::CommandBuffer*>(prepare_list)) {
        require(tags[0].type == sl::kBufferTypeDepth && tags[1].type == sl::kBufferTypeMotionVectors, "prepare uses typed guide tags");
        require(tags[0].extent.width == 640 && tags[0].extent.height == 360 &&
            tags[1].extent == tags[0].extent && tags[1].lifecycle == sl::ResourceLifecycle::eOnlyValidNow,
            "both staged guides carry the render extent and copy lifetime");
    } else {
        require(commands == reinterpret_cast<sl::CommandBuffer*>(color_list) && tags[0].type == sl::kBufferTypeHUDLessColor &&
            tags[1].type == sl::kBufferTypeUIColorAndAlpha && !tags[1].resource, "HUD-less explicitly clears absent UI recomposition input");
        require(tags[0].extent.width == 1280 && tags[0].extent.height == 720 &&
            tags[1].lifecycle == sl::ResourceLifecycle::eValidUntilPresent && !tags[1].extent,
            "HUD-less uses the presented extent and the cleared UI tag has no extent");
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
    require(value == device && used == allocator && !locked && info->imageUsage == expected_chain_usage,
        "native proxy creation retains the allocator and exact WSI usage outside the broker");
    ++chain_creates;
    *out = chain; return refuse_create ? VK_ERROR_INITIALIZATION_FAILED : VK_SUCCESS;
}
void VKAPI_PTR destroy_images(VkDevice value, VkSwapchainKHR actual, const VkAllocationCallbacks* used) {
    require(value == device && actual == chain && used == allocator && !locked, "native proxy destroy uses exact device, chain and allocator");
    ++destroyed_chains;
}
VkResult VKAPI_PTR enumerate_images(VkDevice value, VkSwapchainKHR actual, uint32_t* count, VkImage* images) {
    require(value == device && actual == chain, "enumeration uses the owned proxy");
    if (refuse_enumeration) return VK_ERROR_INITIALIZATION_FAILED;
    if (!images) { *count = backbuffer_count; return VK_SUCCESS; }
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
VkFence waited_fence = VK_NULL_HANDLE, destroyed_fence = VK_NULL_HANDLE;
uint64_t fence_timeout = 0;
VkResult VKAPI_PTR wait_fences(VkDevice, uint32_t, const VkFence* fences, VkBool32, uint64_t timeout) {
    waited_fence = fences[0]; fence_timeout = timeout; return VK_SUCCESS;
}
void VKAPI_PTR destroy_fence(VkDevice, VkFence fence, const VkAllocationCallbacks*) { destroyed_fence = fence; }
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
    *info = {}; info->supportedUsageFlags = surface_usage; return VK_SUCCESS;
}
// Dedicated present capture leaves. The packet functions bypass the SDK proxy.
const auto capture_pool = reinterpret_cast<VkCommandPool>(0x500);
const auto capture_commands = reinterpret_cast<VkCommandBuffer>(0x501);
const auto real_chain = reinterpret_cast<VkSwapchainKHR>(0x33);
uint32_t pool_family = UINT32_MAX, pool_flags = 0, destroyed_pools = 0, capture_begins = 0, capture_ends = 0;
uint32_t wsi_calls = 0, real_presents = 0, real_enumerations = 0, real_submits = 0;
bool in_wsi = false;
const VkPresentInfoKHR* presented_info = nullptr;
VkPresentInfoKHR presented{};
VkSubmitInfo submitted{};
VkPipelineStageFlags submitted_stages[2]{};
VkFence submitted_fence = VK_NULL_HANDLE;
VkCommandBuffer submitted_commands = VK_NULL_HANDLE;
VkResult VKAPI_PTR create_pool(VkDevice value, const VkCommandPoolCreateInfo* info, const VkAllocationCallbacks*, VkCommandPool* pool) {
    require(value == device, "capture pools use the owner device");
    pool_family = info->queueFamilyIndex; pool_flags = info->flags; *pool = capture_pool; return VK_SUCCESS;
}
void VKAPI_PTR destroy_pool(VkDevice, VkCommandPool pool, const VkAllocationCallbacks*) {
    require(pool == capture_pool, "capture destroys its own pool"); ++destroyed_pools;
}
VkResult VKAPI_PTR reset_pool(VkDevice, VkCommandPool pool, VkCommandPoolResetFlags) {
    require(pool == capture_pool, "capture resets its own pool"); return VK_SUCCESS;
}
VkResult VKAPI_PTR allocate_commands(VkDevice, const VkCommandBufferAllocateInfo* info, VkCommandBuffer* commands) {
    require(info->commandPool == capture_pool && info->commandBufferCount == 1 && info->level == VK_COMMAND_BUFFER_LEVEL_PRIMARY,
        "one primary buffer per queue family");
    *commands = capture_commands; return VK_SUCCESS;
}
VkResult VKAPI_PTR begin_commands(VkCommandBuffer commands, const VkCommandBufferBeginInfo* info) {
    require(commands == capture_commands && info->flags == VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT, "copy is recorded once");
    ++capture_begins; return VK_SUCCESS;
}
VkResult VKAPI_PTR end_commands(VkCommandBuffer commands) {
    require(commands == capture_commands, "capture ends its own buffer"); ++capture_ends; return VK_SUCCESS;
}
int32_t wsi_lease(int32_t (*call)(void*), void* user) {
    require(!in_wsi, "WSI lease is not nested");
    in_wsi = true; ++wsi_calls; const auto result = call(user); in_wsi = false; return result;
}
VkResult VKAPI_PTR real_present(VkQueue actual, const VkPresentInfoKHR* info) {
    require(in_wsi && actual == queue, "real Present uses the packet queue under the WSI lease");
    presented_info = info; presented = *info; ++real_presents; return VK_SUBOPTIMAL_KHR;
}
VkResult VKAPI_PTR real_images(VkDevice value, VkSwapchainKHR actual, uint32_t* count, VkImage* images) {
    require(in_wsi && value == device && actual == real_chain, "real image enumeration uses the WSI lease");
    ++real_enumerations;
    if (!images) { *count = 3; return VK_SUCCESS; }
    for (uint32_t i = 0; i < *count; ++i) images[i] = reinterpret_cast<VkImage>(uintptr_t(0x700 + i));
    return VK_SUCCESS;
}
VkResult VKAPI_PTR real_submit(VkQueue actual, uint32_t count, const VkSubmitInfo* info, VkFence fence) {
    require(!in_wsi && actual == queue && count == 1 && info->waitSemaphoreCount <= 2, "copy submits on the packet queue outside WSI");
    submitted = *info; submitted_fence = fence; submitted_commands = info->pCommandBuffers[0]; ++real_submits;
    for (uint32_t i = 0; i < info->waitSemaphoreCount; ++i) submitted_stages[i] = info->pWaitDstStageMask[i];
    return VK_SUCCESS;
}
rsf_sl_present_capture_callback capture_callback = nullptr;
void* capture_user = nullptr;
uint32_t registrations = 0;
int32_t register_capture(const rsf_sl_present_capture_registration* value) {
    require(value->struct_size == sizeof(*value) && value->abi_version == RSF_SL_PRESENT_CAPTURE_ABI_VERSION &&
        value->device == device && value->user && (!capture_user || value->user == capture_user),
        "registration and clearing name the same device and owner");
    capture_callback = value->callback; capture_user = value->user; ++registrations; return VK_SUCCESS;
}

rsf_vk_function device_proc(void*, const char* name) {
#define MOCK(vk, fn) if (std::strcmp(name, #vk) == 0) return reinterpret_cast<rsf_vk_function>(fn)
    MOCK(vkCreateCommandPool, create_pool); MOCK(vkDestroyCommandPool, destroy_pool); MOCK(vkResetCommandPool, reset_pool);
    MOCK(vkAllocateCommandBuffers, allocate_commands); MOCK(vkBeginCommandBuffer, begin_commands); MOCK(vkEndCommandBuffer, end_commands);
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

// Runs the Rust present capture owner with the real Vulkan, WSI and
// registration leaves. Only open, frame selection, readback recording and
// file output are synthetic.
rsf::CaptureVk* synthetic_capture = nullptr;
uint64_t capture_readbacks = 0;
uint32_t capture_flushes = 0, capture_metadata = 0;
const char* capture_failure = nullptr;
int32_t capture_failure_result = 0;
int32_t open_present_capture(const rsf_fgvk_setup& setup, rsf_dlss_capture** capture) {
    static const auto synthetic = [] {
        auto result = rsf::capture_platform;
        result.open = [](const rsf_fgvk_setup*, void** out) -> rsf_backend_result { *out = synthetic_capture; return RSF_BACKEND_OK; };
        result.wants = [](void*, uint64_t frame) -> uint32_t { return frame == 42; };
        result.readbacks = [](void*) -> uint64_t { return capture_readbacks; };
        result.record = [](void*, void* commands, const rsf_srvk_image* image, const rsf_frame_record* record, const char* role) -> uint32_t {
            require(commands == capture_commands && image->device == device && image->image == 0x701 &&
                image->format == VK_FORMAT_B8G8R8A8_UNORM && image->width == 1280 && image->height == 720 &&
                image->layout == VK_IMAGE_LAYOUT_PRESENT_SRC_KHR && image->copy_source && record->frame_id == 42 &&
                std::strcmp(role, "fg-generated-present-2") == 0,
                "readback records the presented real WSI image for the exact source");
            ++capture_readbacks; return 1;
        };
        result.flush = [](void*) -> uint32_t { ++capture_flushes; return 1; };
        result.failure = [](void*, uint64_t, const char* reason, int32_t value) { capture_failure = reason; capture_failure_result = value; };
        result.append = [](void*, const char* file, const uint8_t* bytes, size_t length) {
            if (std::strcmp(file, "dlss-present-capture.jsonl") != 0) return;
            const std::string line(reinterpret_cast<const char*>(bytes), length);
            require(line.find("\"frame\":42,") != std::string::npos &&
                line.find("\"sequence\":1,") != std::string::npos &&
                line.find("\"present_result\":" + std::to_string(VK_SUBOPTIMAL_KHR) + ",") != std::string::npos &&
                line.find("\"legacy_flush_attempted\":true,\"legacy_flush_succeeded\":true,") != std::string::npos &&
                line.find("\"readback_completion_proof\":\"dedicated-capture-fences\"") != std::string::npos,
                "metadata keeps the native Present result and completion proof");
            ++capture_metadata;
        };
        return result;
    }();
    synthetic_capture = new rsf::CaptureVk;
    auto& vk = *synthetic_capture;
    vk.setup = setup; vk.registration = register_capture;
    vk.commands = rsf_vk_capture_commands_open(&vk.setup.graphics);
    require(vk.commands != nullptr, "synthetic capture resolves its command and fence operations");
    waited_fence = VK_NULL_HANDLE;
    return rsf_core_dlss_capture_open(&synthetic, &setup, capture);
}
}
extern "C" rsf_backend_result rsf_slvk_invoke(const rsf_slvk_call* call) {
    require(!locked && call->struct_size == sizeof(*call) && call->abi_version == RSF_SLVK_ABI_VERSION,
        "worker receives versioned call outside the host queue lock");
    if (call->session_id && (call->session_id != lease_session || call->frame_id != lease_frame)) return RSF_BACKEND_ERROR_NOT_READY;
    return call->callback(reinterpret_cast<void*>(1), call->frame_id ? reinterpret_cast<uintptr_t>(&sdk_token) : 0, call->user);
}
extern "C" uint64_t VulkanStorySlWorkerTimeoutEpoch() { return worker_timeout_epoch; }
extern "C" rsf_dlss_fixture_state* rsf_fixture_dlss_state() { return &fixture; }
extern "C" rsf_vk_function rsf_fixture_dlss_proc(void* object, const char* name) { return device_proc(object, name); }
extern "C" int32_t rsf_fixture_dlss_submit(void* user, uint32_t count, const void* commands, uint64_t fence) {
    return submit(user, count, commands, fence);
}
extern "C" void rsf_fixture_dlss_lock(void* user) { lock(user); }
extern "C" void rsf_fixture_dlss_unlock(void* user) { unlock(user); }

extern "C" rsf_backend_result rsf_fixture_dlss_create(rsf_dlss_fixture_variant variant, const rsf_fgvk_setup* setup,
    rsf_dlssgvk** owner, uint64_t* chain_out) {
    // The provider borrows the selected platform table until destruction.
    static const auto synthetic = [] { auto result = platform; result.initialize = fake_initialize; return result; }();
    static const auto capture_required = [] {
        auto result = synthetic;
        result.prepare_chain = [](void* native, uint32_t* capture_enabled, void** gpu) {
            const auto result = platform.prepare_chain(native, capture_enabled, gpu);
            if (result == RSF_BACKEND_OK) *capture_enabled = 1;
            return result;
        };
        return result;
    }();
    static const auto capture_refused = [] {
        auto result = synthetic;
        result.start_capture = [](void*) { return RSF_BACKEND_ERROR_FEATURE_FAILED; };
        return result;
    }();
    const auto& selected = variant == RSF_DLSS_FIXTURE_CAPTURE_REQUIRED ? capture_required :
        variant == RSF_DLSS_FIXTURE_CAPTURE_REFUSED ? capture_refused : synthetic;
    return rsf_core_dlgvk_create(&selected, setup, owner, chain_out);
}
extern "C" int32_t rsf_fixture_dlss_capture_open(const rsf_fgvk_setup* setup, rsf_dlss_capture** capture,
    rsf_dlss_fixture_capture_calls* calls) {
    *calls = {reinterpret_cast<uintptr_t>(real_present), reinterpret_cast<uintptr_t>(real_images),
        reinterpret_cast<uintptr_t>(real_submit), reinterpret_cast<uintptr_t>(wsi_lease)};
    return open_present_capture(*setup, capture);
}
extern "C" void rsf_fixture_dlss_capture_state(rsf_dlss_fixture_capture_state* out) {
    *out = {capture_user, capture_callback ? 1u : 0u, registrations,
        pool_family, pool_flags, capture_begins, capture_ends, destroyed_pools,
        real_submits, real_enumerations, real_presents, wsi_calls,
        reinterpret_cast<uintptr_t>(submitted.pWaitSemaphores), reinterpret_cast<uintptr_t>(submitted_commands),
        reinterpret_cast<uintptr_t>(submitted_fence), reinterpret_cast<uintptr_t>(waited_fence),
        reinterpret_cast<uintptr_t>(destroyed_fence), submitted.waitSemaphoreCount, submitted.commandBufferCount,
        {submitted_stages[0], submitted_stages[1]}, fence_timeout, capture_flushes, capture_metadata,
        reinterpret_cast<uintptr_t>(presented_info), reinterpret_cast<uintptr_t>(presented.pWaitSemaphores),
        reinterpret_cast<uintptr_t>(presented.pNext), reinterpret_cast<uintptr_t>(presented.pSwapchains),
        reinterpret_cast<uintptr_t>(presented.pImageIndices), presented.waitSemaphoreCount, presented.swapchainCount,
        capture_failure, capture_failure_result};
}
extern "C" int32_t rsf_fixture_dlss_capture_call(const rsf_sl_present_capture_packet* packet) {
    return capture_callback(capture_user, packet);
}

int main() {
    SetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", nullptr);
    return rsf_fixture_dlss_run();
}
