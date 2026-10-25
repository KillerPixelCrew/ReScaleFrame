// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_fg_dlss_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include <rescaleframe/native_latency_vulkan.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstring>
#include <limits>
#include <memory>
#include "native_fg_dlss_vulkan_capture.h"

#if defined(RSF_HAVE_STREAMLINE_VK)
#define VK_NO_PROTOTYPES
#include <windows.h>
#include <vulkan/vulkan.h>
#include <sl.h>
#include <sl_dlss_g.h>
#include <DirectXMath.h>
#include "../../backends/common/frame_capture_vk.h"
extern "C" uint64_t VulkanStorySlWorkerTimeoutEpoch();
namespace {
std::atomic<bool> occupied{false};
std::atomic<int32_t> asynchronous_error{0};
const sl::ViewportHandle viewport(0);
struct Image {
    VkImage image = VK_NULL_HANDLE; VkDeviceMemory memory = VK_NULL_HANDLE;
    VkImageView view = VK_NULL_HANDLE;
    uint32_t width = 0, height = 0; VkFormat format = VK_FORMAT_UNDEFINED;
    bool initialized = false;
};
struct Slot {
    rsf_frame_record record{};
    uint64_t recording_sequence = 0;
    void* prepare_commands = nullptr; void* hudless_commands = nullptr;
    bool prepared = false, hudless = false, submitted_prepare = false, submitted_hudless = false;
    bool recorded_prepare = false, recorded_hudless = false;
    bool source_pending = false, consumer_pending = false, completion_unknown = false;
    bool sdk_inputs_pending = false;
    VkSemaphore sdk_inputs_completion = VK_NULL_HANDLE;
    uint64_t sdk_inputs_value = 0;
    VkFence source_completion = VK_NULL_HANDLE, consumer_completion = VK_NULL_HANDLE;
    Image depth, motion, color;
};
template<class T> T entry(void* module, const char* name) { return reinterpret_cast<T>(GetProcAddress(static_cast<HMODULE>(module), name)); }
sl::float4x4 matrix(const float* values) {
    sl::float4x4 result{};
    for (uint32_t row = 0; row < 4; ++row) for (uint32_t column = 0; column < 4; ++column)
        (&result[row].x)[column] = values[row * 4 + column];
    return result;
}
void on_error(const sl::APIError& error) { asynchronous_error = static_cast<int32_t>(error.vkRes); }
}
struct rsf_dlssgvk {
    rsf_fgvk_setup setup{};
    VkSwapchainKHR chain = VK_NULL_HANDLE;
    VkFormat format = VK_FORMAT_UNDEFINED;
    uint32_t width = 0, height = 0, backbuffers = 0, count = 1, configured_count = 0;
    bool claimed = false, requested = false, enabled = false, resources = false;
    uint64_t latest = 0, presented = 0, actual_presents = 0, recording_sequence = 0, presented_sequence = 0;
    std::array<Slot, 6> slots{};
    VkPhysicalDeviceMemoryProperties memory{};
    PFN_vkCreateSwapchainKHR create_chain = nullptr; PFN_vkDestroySwapchainKHR destroy_chain = nullptr;
    PFN_vkGetSwapchainImagesKHR images = nullptr; PFN_vkAcquireNextImageKHR acquire = nullptr;
    PFN_vkQueuePresentKHR present = nullptr;
    PFN_vkCreateImage create_image = nullptr; PFN_vkDestroyImage destroy_image = nullptr;
    PFN_vkGetImageMemoryRequirements requirements = nullptr; PFN_vkAllocateMemory allocate = nullptr;
    PFN_vkFreeMemory free_memory = nullptr; PFN_vkBindImageMemory bind = nullptr;
    PFN_vkCreateImageView create_view = nullptr; PFN_vkDestroyImageView destroy_view = nullptr;
    PFN_vkCmdPipelineBarrier barrier = nullptr; PFN_vkCmdCopyImage copy = nullptr;
    PFN_vkDeviceWaitIdle idle = nullptr; PFN_vkCreateFence create_fence = nullptr;
    PFN_vkDestroyFence destroy_fence = nullptr; PFN_vkResetFences reset_fences = nullptr;
    PFN_vkWaitForFences wait_fences = nullptr;
    PFN_vkWaitSemaphores wait_semaphores = nullptr;
    VkSemaphore last_inputs_completion = VK_NULL_HANDLE;
    uint64_t last_inputs_value = 0;
    PFun_slSetConstants* constants = nullptr; PFun_slSetTagForFrame* tags = nullptr;
    PFun_slFreeResources* release = nullptr;
    PFun_slDLSSGSetOptions* options = nullptr; PFun_slDLSSGGetState* state = nullptr;
    rsf_dlssgvk_status status{sizeof(status), RSF_DLSSGVK_ABI_VERSION,
        {sizeof(rsf_fgvk_status), RSF_FGVK_ABI_VERSION}};
    rsf::FrameCaptureVk capture;
    rsf::DlssPresentCapture present_capture;
    void free_image(Image& image) {
        const auto device = static_cast<VkDevice>(setup.graphics.device);
        if (image.view) destroy_view(device, image.view, nullptr);
        if (image.image) destroy_image(device, image.image, nullptr);
        if (image.memory) free_memory(device, image.memory, nullptr);
        image = {};
    }
    ~rsf_dlssgvk() {
        const auto device = static_cast<VkDevice>(setup.graphics.device);
        for (auto& slot : slots) {
            for (auto* image : {&slot.depth, &slot.motion, &slot.color}) free_image(*image);
            if (slot.source_completion) destroy_fence(device, slot.source_completion, nullptr);
            if (slot.consumer_completion) destroy_fence(device, slot.consumer_completion, nullptr);
        }
        if (claimed) occupied = false;
    }
};
namespace {
struct QueueLock {
    rsf_dlssgvk& self;
    explicit QueueLock(rsf_dlssgvk& value) : self(value) { self.setup.lock_queues(self.setup.queue_user); }
    ~QueueLock() { self.setup.unlock_queues(self.setup.queue_user); }
};
rsf_backend_result sdk_result(rsf_dlssgvk& self, sl::Result result) {
    self.status.sdk_result = static_cast<int32_t>(result);
    return result == sl::Result::eOk ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
rsf_backend_result query_state(rsf_dlssgvk& self) {
    sl::DLSSGState state{};
    const auto result = sdk_result(self, self.state(viewport, state, nullptr));
    if (result != RSF_BACKEND_OK) return result;
    self.actual_presents += state.numFramesActuallyPresented;
    self.status.presentation.last_present_count = self.actual_presents;
    self.status.max_generated_frames = state.numFramesToGenerateMax;
    self.status.sdk_status = static_cast<uint32_t>(state.status);
    self.status.min_dimension = state.minWidthOrHeight;
    self.status.vsync_supported = state.bIsVsyncSupportAvailable == sl::Boolean::eTrue;
    self.status.dynamic_mfg_supported = state.bIsDynamicMFGSupported == sl::Boolean::eTrue;
    // Borrowed from the one live SDK owner. Capture on the presenting thread,
    // retain per submitted source, and retire before freeing feature resources.
    self.last_inputs_completion = static_cast<VkSemaphore>(state.inputsProcessingCompletionFence);
    self.last_inputs_value = state.lastPresentInputsProcessingCompletionFenceValue;
    return RSF_BACKEND_OK;
}
rsf_backend_result wait_inputs(rsf_dlssgvk& self, Slot& slot) {
    if (!slot.sdk_inputs_pending) return RSF_BACKEND_OK;
    if (!slot.sdk_inputs_completion || !self.wait_semaphores) return RSF_BACKEND_ERROR_NOT_READY;
    VkSemaphoreWaitInfo info{}; info.sType = VK_STRUCTURE_TYPE_SEMAPHORE_WAIT_INFO;
    info.semaphoreCount = 1; info.pSemaphores = &slot.sdk_inputs_completion; info.pValues = &slot.sdk_inputs_value;
    if (self.wait_semaphores(static_cast<VkDevice>(self.setup.graphics.device), &info, 15000000000ull) != VK_SUCCESS)
        return RSF_BACKEND_ERROR_NOT_READY;
    slot.sdk_inputs_pending = false;
    return RSF_BACKEND_OK;
}
rsf_backend_result initialize(void* module, uintptr_t, void* user) {
    auto& self = *static_cast<rsf_dlssgvk*>(user);
    auto supported = entry<PFun_slIsFeatureSupported*>(module, "slIsFeatureSupported");
    auto feature = entry<PFun_slGetFeatureFunction*>(module, "slGetFeatureFunction");
    self.constants = entry<PFun_slSetConstants*>(module, "slSetConstants");
    self.tags = entry<PFun_slSetTagForFrame*>(module, "slSetTagForFrame");
    self.release = entry<PFun_slFreeResources*>(module, "slFreeResources");
    if (!supported || !feature || !self.constants || !self.tags || !self.release) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    sl::AdapterInfo adapter{}; adapter.vkPhysicalDevice = self.setup.graphics.physical_device;
    const auto support = supported(sl::kFeatureDLSS_G, adapter);
    self.status.sdk_result = static_cast<int32_t>(support);
    if (support != sl::Result::eOk) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    void* options = nullptr; void* state = nullptr;
    if (feature(sl::kFeatureDLSS_G, "slDLSSGSetOptions", options) != sl::Result::eOk ||
        feature(sl::kFeatureDLSS_G, "slDLSSGGetState", state) != sl::Result::eOk || !options || !state)
        return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    self.options = reinterpret_cast<PFun_slDLSSGSetOptions*>(options);
    self.state = reinterpret_cast<PFun_slDLSSGGetState*>(state);
    const auto result = query_state(self);
    if (result != RSF_BACKEND_OK) return result;
    if (!self.status.max_generated_frames || self.width < self.status.min_dimension || self.height < self.status.min_dimension)
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    return RSF_BACKEND_OK;
}
rsf_backend_result configure(rsf_dlssgvk& self, bool enabled) {
    if (self.enabled == enabled && (!enabled || self.configured_count == self.count)) return RSF_BACKEND_OK;
    sl::DLSSGOptions options{}; options.mode = enabled ? sl::DLSSGMode::eOn : sl::DLSSGMode::eOff;
    options.flags = sl::DLSSGFlags::eRetainResourcesWhenOff;
    options.numFramesToGenerate = enabled ? self.count : 1;
    options.colorWidth = self.width; options.colorHeight = self.height;
    options.colorBufferFormat = self.format; options.numBackBuffers = self.backbuffers;
    options.depthBufferFormat = VK_FORMAT_R32_SFLOAT; options.mvecBufferFormat = VK_FORMAT_R16G16_SFLOAT;
    options.hudLessBufferFormat = VK_FORMAT_R8G8B8A8_UNORM;
    options.queueParallelismMode = sl::DLSSGQueueParallelismMode::eBlockNoClientQueues;
    // No UI alpha/color producer is supplied by this ABI. HUD-less color alone
    // supports the SDK's ordinary UI detection, not separate UI recomposition.
    options.enableUserInterfaceRecomposition = sl::Boolean::eFalse;
    options.onErrorCallback = on_error;
    const auto result = sdk_result(self, self.options(viewport, options));
    if (result == RSF_BACKEND_OK) {
        self.enabled = enabled; self.configured_count = self.count;
        self.resources |= enabled;
    }
    return result;
}
rsf_backend_result disable(void*, uintptr_t, void* user) { return configure(*static_cast<rsf_dlssgvk*>(user), false); }
rsf_backend_result invoke(rsf_dlssgvk& self, rsf_slvk_callback callback, uint64_t session = 0, uint64_t frame = 0, void* user = nullptr) {
    const rsf_slvk_call call{sizeof(call), RSF_SLVK_ABI_VERSION, session, frame, callback, user ? user : &self};
    return rsf_slvk_invoke(&call);
}
bool allocate(rsf_dlssgvk& self, Image& image, uint32_t width, uint32_t height, VkFormat format) {
    if (image.image && image.width == width && image.height == height && image.format == format) return true;
    self.free_image(image);
    const auto device = static_cast<VkDevice>(self.setup.graphics.device);
    VkImageCreateInfo info{}; info.sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO; info.imageType = VK_IMAGE_TYPE_2D;
    info.extent = {width, height, 1}; info.mipLevels = info.arrayLayers = 1; info.format = format;
    info.samples = VK_SAMPLE_COUNT_1_BIT; info.tiling = VK_IMAGE_TILING_OPTIMAL;
    info.usage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    if (self.create_image(device, &info, nullptr, &image.image) != VK_SUCCESS) return false;
    VkMemoryRequirements requirements{}; self.requirements(device, image.image, &requirements);
    uint32_t type = UINT32_MAX;
    for (uint32_t i = 0; i < self.memory.memoryTypeCount; ++i) if ((requirements.memoryTypeBits & (1u << i)) &&
        (self.memory.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT)) { type = i; break; }
    if (type == UINT32_MAX) return false;
    VkMemoryAllocateInfo allocation{}; allocation.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO;
    allocation.allocationSize = requirements.size; allocation.memoryTypeIndex = type;
    if (self.allocate(device, &allocation, nullptr, &image.memory) != VK_SUCCESS || self.bind(device, image.image, image.memory, 0) != VK_SUCCESS) return false;
    VkImageViewCreateInfo view{}; view.sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
    view.image = image.image; view.viewType = VK_IMAGE_VIEW_TYPE_2D; view.format = format;
    view.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    if (self.create_view(device, &view, nullptr, &image.view) != VK_SUCCESS) return false;
    image.width = width; image.height = height; image.format = format;
    return true;
}
void transition(rsf_dlssgvk& self, VkCommandBuffer commands, VkImage image, VkImageLayout before, VkImageLayout after) {
    VkImageMemoryBarrier barrier{}; barrier.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
    barrier.image = image; barrier.oldLayout = before; barrier.newLayout = after;
    barrier.srcAccessMask = before == VK_IMAGE_LAYOUT_UNDEFINED ? 0 : VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    barrier.dstAccessMask = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    barrier.srcQueueFamilyIndex = barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    self.barrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &barrier);
}
void stage(rsf_dlssgvk& self, VkCommandBuffer commands, const rsf_srvk_image& source, Image& destination) {
    const auto image = reinterpret_cast<VkImage>(source.image);
    transition(self, commands, image, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL);
    transition(self, commands, destination.image, destination.initialized ? VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL : VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
    VkImageCopy region{}; region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1}; region.extent = {source.width, source.height, 1};
    self.copy(commands, image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, destination.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
    transition(self, commands, image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
    transition(self, commands, destination.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
    destination.initialized = true;
}
sl::Resource resource(const Image& image) {
    sl::Resource result{sl::ResourceType::eTex2d, reinterpret_cast<void*>(image.image), nullptr,
        reinterpret_cast<void*>(image.view), VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL};
    result.width = image.width; result.height = image.height; result.nativeFormat = image.format;
    result.mipLevels = result.arrayLayers = 1;
    result.usage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    return result;
}
bool valid_record(const rsf_dlssgvk& self, const rsf_fgvk_frame& frame) {
    if (!frame.record) return false;
    const auto& record = *frame.record; const auto& camera = record.camera;
    if (record.struct_size < sizeof(record) || record.abi_version != RSF_GAME_FRAME_ABI_VERSION ||
        camera.struct_size < sizeof(camera) || camera.abi_version != RSF_GAME_FRAME_ABI_VERSION) return false;
    for (const auto* values : {camera.view_to_clip, camera.clip_to_view, camera.clip_to_previous_clip, camera.view_to_world})
        for (uint32_t i = 0; i < 16; ++i) if (!std::isfinite(values[i])) return false;
    return record.session_id && record.frame_id && record.render_width && record.render_height &&
        record.render_width <= self.width && record.render_height <= self.height && record.output_width == self.width && record.output_height == self.height &&
        camera.render_width == record.render_width && camera.render_height == record.render_height && camera.output_width == self.width && camera.output_height == self.height &&
        std::isfinite(record.frame_time_ms) && record.frame_time_ms > 0 && std::isfinite(camera.near_plane) && camera.near_plane > 0 &&
        std::isfinite(camera.far_plane) && camera.far_plane >= 0 && (self.setup.graphics.depth_infinite || camera.far_plane > camera.near_plane) &&
        std::isfinite(camera.vertical_fov_radians) && camera.vertical_fov_radians > 0 && camera.vertical_fov_radians < 3.141593f &&
        std::isfinite(camera.jitter_pixels[0]) && std::isfinite(camera.jitter_pixels[1]) &&
        camera.depth_inverted == self.setup.graphics.depth_inverted && camera.motion_jittered == self.setup.graphics.motion_jittered;
}
bool valid_image(const rsf_dlssgvk& self, const rsf_srvk_image& image, uint32_t width, uint32_t height, VkFormat format) {
    return image.device == self.setup.graphics.device && image.image && image.copy_source && image.width == width && image.height == height &&
        image.format == uint32_t(format) && image.layout == VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
}
struct RecordCall { rsf_dlssgvk& self; const rsf_fgvk_frame& input; VkCommandBuffer commands; Slot& slot; };
rsf_backend_result prepare(void*, uintptr_t token, void* user) {
    auto& call = *static_cast<RecordCall*>(user); auto& self = call.self; auto& slot = call.slot;
    const auto& input = call.input; const auto& camera = input.record->camera;
    if (!token) return RSF_BACKEND_ERROR_NOT_READY;
    DirectX::XMFLOAT4X4 previous{}, inverse{}; std::memcpy(&previous, camera.clip_to_previous_clip, sizeof(previous));
    DirectX::XMVECTOR determinant{};
    DirectX::XMStoreFloat4x4(&inverse, DirectX::XMMatrixInverse(&determinant, DirectX::XMLoadFloat4x4(&previous)));
    const auto det = DirectX::XMVectorGetX(determinant);
    if (!std::isfinite(det) || std::abs(det) < 1e-12f) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    sl::Constants constants{}; constants.cameraViewToClip = matrix(camera.view_to_clip); constants.clipToCameraView = matrix(camera.clip_to_view);
    constants.clipToPrevClip = matrix(camera.clip_to_previous_clip); constants.prevClipToClip = matrix(&inverse.m[0][0]);
    constants.cameraPos = {camera.view_to_world[12], camera.view_to_world[13], camera.view_to_world[14]};
    constants.cameraRight = {camera.view_to_world[0], camera.view_to_world[1], camera.view_to_world[2]};
    constants.cameraUp = {camera.view_to_world[4], camera.view_to_world[5], camera.view_to_world[6]};
    constants.cameraFwd = {-camera.view_to_world[8], -camera.view_to_world[9], -camera.view_to_world[10]};
    constants.cameraNear = camera.near_plane; constants.cameraFar = camera.far_plane > 0 ? camera.far_plane : std::numeric_limits<float>::max();
    constants.cameraFOV = camera.vertical_fov_radians; constants.cameraAspectRatio = float(self.width) / self.height;
    constants.jitterOffset = {camera.jitter_pixels[0], camera.jitter_pixels[1]}; constants.cameraPinholeOffset = {0, 0};
    // The adapter's scale already accounts for the engine motion convention.
    // As in DLSS SR, convert its pixel scale to normalized texture coordinates.
    constants.mvecScale = {input.motion_scale_x / slot.record.render_width, input.motion_scale_y / slot.record.render_height};
    constants.depthInverted = camera.depth_inverted ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    constants.cameraMotionIncluded = sl::Boolean::eTrue; constants.motionVectors3D = sl::Boolean::eFalse;
    constants.motionVectorsJittered = camera.motion_jittered ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    constants.motionVectorsInvalidValue = std::numeric_limits<float>::max();
    constants.reset = (slot.record.flags & RSF_FRAME_FLAG_RESET) ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    auto& frame = *reinterpret_cast<sl::FrameToken*>(token);
    auto result = sdk_result(self, self.constants(constants, frame, viewport));
    if (result != RSF_BACKEND_OK) return result;
    stage(self, call.commands, input.depth, slot.depth); stage(self, call.commands, input.motion, slot.motion);
    slot.recorded_prepare = true;
    auto depth = resource(slot.depth); auto motion = resource(slot.motion);
    const sl::Extent render{0, 0, slot.record.render_width, slot.record.render_height};
    // Engine guides are overwritten before Present. The SDK copies on this
    // recording list so the original CPU token owns the exact source frame.
    sl::ResourceTag tags[] = {{&depth, sl::kBufferTypeDepth, sl::ResourceLifecycle::eOnlyValidNow, &render},
        {&motion, sl::kBufferTypeMotionVectors, sl::ResourceLifecycle::eOnlyValidNow, &render}};
    result = sdk_result(self, self.tags(frame, viewport, tags, 2, reinterpret_cast<sl::CommandBuffer*>(call.commands)));
    if (result != RSF_BACKEND_OK) return result;
    slot.prepared = true; ++self.status.presentation.prepared;
    (void)self.capture.record(call.commands, input.depth, slot.record, "fg-depth", slot.record.flags & RSF_FRAME_FLAG_RESET);
    (void)self.capture.record(call.commands, input.motion, slot.record, "fg-motion-uv", slot.record.flags & RSF_FRAME_FLAG_RESET);
    return RSF_BACKEND_OK;
}
rsf_backend_result hudless(void*, uintptr_t token, void* user) {
    auto& call = *static_cast<RecordCall*>(user); auto& self = call.self; auto& slot = call.slot;
    if (!token) return RSF_BACKEND_ERROR_NOT_READY;
    stage(self, call.commands, call.input.hudless, slot.color);
    slot.recorded_hudless = true;
    auto color = resource(slot.color); const sl::Extent extent{0, 0, self.width, self.height};
    sl::ResourceTag tags[] = {{&color, sl::kBufferTypeHUDLessColor, sl::ResourceLifecycle::eOnlyValidNow, &extent},
        {nullptr, sl::kBufferTypeUIColorAndAlpha, sl::ResourceLifecycle::eValidUntilPresent, nullptr}};
    const auto result = sdk_result(self, self.tags(*reinterpret_cast<sl::FrameToken*>(token), viewport, tags, 2, reinterpret_cast<sl::CommandBuffer*>(call.commands)));
    if (result != RSF_BACKEND_OK) return result;
    slot.hudless = true; ++self.status.presentation.tagged;
    (void)self.capture.record(call.commands, call.input.hudless, slot.record, "fg-hudless-color", slot.record.flags & RSF_FRAME_FLAG_RESET);
    return RSF_BACKEND_OK;
}
rsf_backend_result release_resources(void*, uintptr_t, void* user) {
    auto& self = *static_cast<rsf_dlssgvk*>(user);
    if (!self.resources) return RSF_BACKEND_OK;
    const auto result = sdk_result(self, self.release(sl::kFeatureDLSS_G, viewport));
    if (result == RSF_BACKEND_OK) self.resources = false;
    return result;
}
}
#else
struct rsf_dlssgvk {};
#endif

extern "C" rsf_backend_result rsf_dlssgvk_create(const rsf_fgvk_setup* setup, rsf_dlssgvk** out, uint64_t* chain) try {
    if (out) *out = nullptr;
    if (chain) *chain = 0;
    if (!setup || !out || !chain || setup->struct_size < sizeof(*setup) || setup->graphics.struct_size < sizeof(setup->graphics)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_FGVK_ABI_VERSION || setup->graphics.abi_version != RSF_SRVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    const auto& graphics = setup->graphics;
    if (!graphics.instance || !graphics.physical_device || !graphics.device || !graphics.instance_proc || !graphics.device_proc ||
        !setup->create_info || !setup->queues[0] || !setup->graphics_submit || !setup->lock_queues || !setup->unlock_queues || !graphics.output_width || !graphics.output_height)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_STREAMLINE_VK)
    auto info = *static_cast<const VkSwapchainCreateInfoKHR*>(setup->create_info);
    if (info.sType != VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR || !info.surface ||
        info.imageExtent.width != graphics.output_width || info.imageExtent.height != graphics.output_height ||
        (info.imageFormat != VK_FORMAT_B8G8R8A8_UNORM && info.imageFormat != VK_FORMAT_R8G8B8A8_UNORM) || info.imageArrayLayers != 1)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto self = std::make_unique<rsf_dlssgvk>(); self->setup = *setup;
    if (occupied.exchange(true)) return RSF_BACKEND_ERROR_NOT_READY;
    self->claimed = true; self->width = graphics.output_width; self->height = graphics.output_height; self->format = info.imageFormat;
    auto result = invoke(*self, initialize); if (result != RSF_BACKEND_OK) return result;
#define RSF_LOAD(member, function) self->member = reinterpret_cast<PFN_##function>(graphics.device_proc(graphics.device, #function)); if (!self->member) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT
    RSF_LOAD(create_chain, vkCreateSwapchainKHR); RSF_LOAD(destroy_chain, vkDestroySwapchainKHR);
    RSF_LOAD(images, vkGetSwapchainImagesKHR); RSF_LOAD(acquire, vkAcquireNextImageKHR); RSF_LOAD(present, vkQueuePresentKHR);
    RSF_LOAD(create_image, vkCreateImage); RSF_LOAD(destroy_image, vkDestroyImage); RSF_LOAD(requirements, vkGetImageMemoryRequirements);
    RSF_LOAD(allocate, vkAllocateMemory); RSF_LOAD(free_memory, vkFreeMemory); RSF_LOAD(bind, vkBindImageMemory);
    RSF_LOAD(create_view, vkCreateImageView); RSF_LOAD(destroy_view, vkDestroyImageView); RSF_LOAD(barrier, vkCmdPipelineBarrier); RSF_LOAD(copy, vkCmdCopyImage);
    RSF_LOAD(idle, vkDeviceWaitIdle); RSF_LOAD(create_fence, vkCreateFence); RSF_LOAD(destroy_fence, vkDestroyFence);
    RSF_LOAD(reset_fences, vkResetFences); RSF_LOAD(wait_fences, vkWaitForFences);
    RSF_LOAD(wait_semaphores, vkWaitSemaphores);
#undef RSF_LOAD
    auto properties = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(graphics.instance_proc(graphics.instance, "vkGetPhysicalDeviceMemoryProperties"));
    if (!properties) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    properties(static_cast<VkPhysicalDevice>(graphics.physical_device), &self->memory);
    self->capture.set_provider(0, RSF_SR_DLSS, true);
    if (!self->capture.initialize(static_cast<VkDevice>(graphics.device), graphics.device_proc, self->memory)) return RSF_BACKEND_ERROR_INIT_FAILED;
    // DLSS copies completed/generated frames into the real WSI images. Their
    // creation usage must allow that destination even without diagnostics.
    const auto capabilities = reinterpret_cast<PFN_vkGetPhysicalDeviceSurfaceCapabilitiesKHR>(graphics.instance_proc(graphics.instance, "vkGetPhysicalDeviceSurfaceCapabilitiesKHR"));
    VkSurfaceCapabilitiesKHR supported{};
    const VkImageUsageFlags required_usage = VK_IMAGE_USAGE_TRANSFER_DST_BIT |
        (self->capture.enabled() ? VK_IMAGE_USAGE_TRANSFER_SRC_BIT : 0);
    if (!capabilities || capabilities(static_cast<VkPhysicalDevice>(graphics.physical_device), info.surface, &supported) != VK_SUCCESS ||
        (supported.supportedUsageFlags & required_usage) != required_usage) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    info.imageUsage |= required_usage;
    // Streamline's LL2 create/destroy hooks acquire their own swapchain mutex.
    // The outer WSI broker is only for image/acquire/present operations.
    const auto created = self->create_chain(static_cast<VkDevice>(self->setup.graphics.device),
        &info, static_cast<const VkAllocationCallbacks*>(self->setup.allocator), &self->chain);
    if (created != VK_SUCCESS) return RSF_BACKEND_ERROR_INIT_FAILED;
    // From this point failure must retire the checked proxy chain explicitly.
    const auto images = rsf_dlssgvk_images(self.get(), &self->backbuffers, nullptr);
    const auto capture_result = images == VK_SUCCESS ? self->present_capture.initialize(*setup) : RSF_BACKEND_ERROR_INIT_FAILED;
    if (images != VK_SUCCESS || capture_result != RSF_BACKEND_OK) {
        // No rendering or SDK feature exists yet, so synchronous destruction
        // cannot race submitted frame work and no partial owner escapes.
        self->destroy_chain(static_cast<VkDevice>(self->setup.graphics.device), self->chain,
            static_cast<const VkAllocationCallbacks*>(self->setup.allocator));
        self->chain = VK_NULL_HANDLE;
        return capture_result;
    }
    self->status.presentation.available = 1; self->status.requested_generated_frames = 1;
    asynchronous_error = 0; *chain = reinterpret_cast<uint64_t>(self->chain); *out = self.release(); return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

extern "C" int32_t rsf_dlssgvk_images(rsf_dlssgvk* self, uint32_t* count, uint64_t* images) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (!self || !count) return VK_ERROR_INITIALIZATION_FAILED;
    return self->images(static_cast<VkDevice>(self->setup.graphics.device), self->chain, count, reinterpret_cast<VkImage*>(images));
#else
    (void)self; (void)count; (void)images; return -3;
#endif
}
extern "C" int32_t rsf_dlssgvk_acquire(rsf_dlssgvk* self, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (!self || !index) return VK_ERROR_INITIALIZATION_FAILED;
    return self->acquire(static_cast<VkDevice>(self->setup.graphics.device), self->chain, timeout,
        reinterpret_cast<VkSemaphore>(semaphore), reinterpret_cast<VkFence>(fence), index);
#else
    (void)self; (void)timeout; (void)semaphore; (void)fence; (void)index; return -3;
#endif
}
extern "C" rsf_backend_result rsf_dlssgvk_enable(rsf_dlssgvk* self, uint32_t enabled, uint32_t count) {
    if (!self || enabled > 1 || !count) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (count > self->status.max_generated_frames) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    self->requested = enabled != 0; self->count = count; self->status.requested_generated_frames = count;
    return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
extern "C" rsf_backend_result rsf_dlssgvk_image_layout(const rsf_dlssgvk* self, uint32_t* layout) {
    if (!self || !layout) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_STREAMLINE_VK)
    // These are SDK-owned game-facing images. Its Vulkan copyResource reads
    // them in TRANSFER_SRC even while FG is Off; real WSI images use Present.
    *layout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL; return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
extern "C" rsf_backend_result rsf_dlssgvk_prepare(rsf_dlssgvk* self, void* commands, const rsf_fgvk_frame* input) try {
    if (!self || !commands || !input || input->struct_size < sizeof(*input)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (input->abi_version != RSF_FGVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (!valid_record(*self, *input)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!self->requested) return RSF_BACKEND_OK;
    const auto& record = *input->record;
    if (!valid_image(*self, input->depth, record.render_width, record.render_height, VK_FORMAT_R32_SFLOAT) ||
        !valid_image(*self, input->motion, record.render_width, record.render_height, VK_FORMAT_R16G16_SFLOAT) ||
        !std::isfinite(input->motion_scale_x) || !std::isfinite(input->motion_scale_y) || !input->motion_scale_x || !input->motion_scale_y)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& slot = self->slots[record.frame_id % self->slots.size()];
    if (slot.completion_unknown || (slot.recorded_prepare && !slot.submitted_prepare) || (slot.recorded_hudless && !slot.submitted_hudless)) return RSF_BACKEND_ERROR_NOT_READY;
    if (wait_inputs(*self, slot) != RSF_BACKEND_OK) return RSF_BACKEND_ERROR_NOT_READY;
    const auto device = static_cast<VkDevice>(self->setup.graphics.device);
    for (auto fence : {slot.source_pending ? slot.source_completion : VK_NULL_HANDLE, slot.consumer_pending ? slot.consumer_completion : VK_NULL_HANDLE})
        if (fence && self->wait_fences(device, 1, &fence, VK_TRUE, 15000000000ull) != VK_SUCCESS) return RSF_BACKEND_ERROR_NOT_READY;
    slot.source_pending = slot.consumer_pending = false;
    VkFenceCreateInfo info{}; info.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO;
    for (auto* fence : {&slot.source_completion, &slot.consumer_completion}) if (!*fence && self->create_fence(device, &info, nullptr, fence) != VK_SUCCESS)
        return RSF_BACKEND_ERROR_INIT_FAILED;
    if (!allocate(*self, slot.depth, record.render_width, record.render_height, VK_FORMAT_R32_SFLOAT) ||
        !allocate(*self, slot.motion, record.render_width, record.render_height, VK_FORMAT_R16G16_SFLOAT) ||
        !allocate(*self, slot.color, self->width, self->height, VK_FORMAT_R8G8B8A8_UNORM)) return RSF_BACKEND_ERROR_INIT_FAILED;
    const auto& previous = self->slots[self->latest % self->slots.size()].record;
    const bool reset = self->latest + 1 != record.frame_id || previous.session_id != record.session_id ||
        previous.view_id != record.view_id || previous.resource_generation != record.resource_generation;
    slot.record = record; slot.recording_sequence = ++self->recording_sequence;
    if (reset) slot.record.flags |= RSF_FRAME_FLAG_RESET;
    slot.prepared = slot.hudless = slot.submitted_prepare = slot.submitted_hudless = false;
    slot.recorded_prepare = slot.recorded_hudless = false;
    slot.prepare_commands = commands; slot.hudless_commands = nullptr;
    RecordCall call{*self, *input, static_cast<VkCommandBuffer>(commands), slot};
    const auto result = invoke(*self, prepare, record.session_id, record.frame_id, &call);
    if (result == RSF_BACKEND_OK) { self->latest = record.frame_id; self->present_capture.remember(slot.record, self->count); }
    self->status.presentation.result = result; return result;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" rsf_backend_result rsf_dlssgvk_hudless(rsf_dlssgvk* self, void* commands, const rsf_fgvk_frame* input) try {
    if (!self || !commands || !input || input->struct_size < sizeof(*input)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (input->abi_version != RSF_FGVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (!valid_record(*self, *input)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!self->requested) return RSF_BACKEND_OK;
    const auto& record = *input->record; auto& slot = self->slots[record.frame_id % self->slots.size()];
    if (!slot.prepared || slot.record.frame_id != record.frame_id || slot.record.session_id != record.session_id ||
        slot.record.view_id != record.view_id || slot.record.resource_generation != record.resource_generation ||
        !valid_image(*self, input->hudless, self->width, self->height, VK_FORMAT_R8G8B8A8_UNORM)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    slot.hudless_commands = commands;
    RecordCall call{*self, *input, static_cast<VkCommandBuffer>(commands), slot};
    const auto result = invoke(*self, hudless, record.session_id, record.frame_id, &call);
    self->status.presentation.result = result; return result;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" void rsf_dlssgvk_submitted(rsf_dlssgvk* self, void* const* commands, uint32_t count) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (!self || !commands) return;
    // A reset/re-recorded command buffer can reuse an aborted frame's pointer.
    // Its newest source owns that recording. Never promote abandoned records
    // merely because their old command-buffer address appears in a later submit.
    for (uint32_t i = 0; i < count; ++i) {
        uint64_t newest = 0;
        for (const auto& slot : self->slots) if ((slot.recorded_prepare && slot.prepare_commands == commands[i] && !slot.submitted_prepare) ||
            (slot.recorded_hudless && slot.hudless_commands == commands[i] && !slot.submitted_hudless)) newest = (std::max)(newest, slot.recording_sequence);
        for (auto& slot : self->slots) if (slot.recording_sequence < newest) {
            if (slot.prepare_commands == commands[i] && !slot.submitted_prepare) { slot.prepared = slot.recorded_prepare = false; slot.prepare_commands = nullptr; }
            if (slot.hudless_commands == commands[i] && !slot.submitted_hudless) { slot.hudless = slot.recorded_hudless = false; slot.hudless_commands = nullptr; }
        }
    }
    for (auto& slot : self->slots) {
        bool changed = false;
        for (uint32_t i = 0; i < count; ++i) {
            if (slot.recorded_prepare && slot.prepare_commands == commands[i] && !slot.submitted_prepare) { slot.submitted_prepare = true; changed = true; }
            if (slot.recorded_hudless && slot.hudless_commands == commands[i] && !slot.submitted_hudless) { slot.submitted_hudless = true; changed = true; }
        }
        if (changed && !slot.completion_unknown) {
            const auto device = static_cast<VkDevice>(self->setup.graphics.device);
            // Prepare and HUD-less may submit separately, and an aborted frame
            // can submit just one. Cover every accepted partial submission.
            if ((slot.source_pending && self->wait_fences(device, 1, &slot.source_completion, VK_TRUE, 15000000000ull) != VK_SUCCESS) ||
                self->reset_fences(device, 1, &slot.source_completion) != VK_SUCCESS || self->setup.graphics_submit(self->setup.queue_user,
                0, nullptr, reinterpret_cast<uint64_t>(slot.source_completion)) != VK_SUCCESS) slot.completion_unknown = true;
            else slot.source_pending = true;
        }
    }
#else
    (void)self; (void)commands; (void)count;
#endif
}
extern "C" int32_t rsf_dlssgvk_present(rsf_dlssgvk* self, const void* present_info) try {
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (!self || !present_info) return VK_ERROR_INITIALIZATION_FAILED;
    const auto& info = *static_cast<const VkPresentInfoKHR*>(present_info);
    if (info.swapchainCount != 1 || !info.pSwapchains || info.pSwapchains[0] != self->chain) return VK_ERROR_INITIALIZATION_FAILED;
    auto& slot = self->slots[self->latest % self->slots.size()];
    const bool candidate = self->requested && slot.recording_sequence > self->presented_sequence && slot.record.frame_id == self->latest &&
        slot.prepared && slot.hudless && slot.submitted_prepare && slot.submitted_hudless && !slot.completion_unknown;
    struct Configure { rsf_dlssgvk& self; bool enabled; } call{*self, candidate};
    auto configured = invoke(*self, [](void*, uintptr_t, void* user) -> rsf_backend_result {
        auto& value = *static_cast<Configure*>(user); return configure(value.self, value.enabled);
    }, candidate ? slot.record.session_id : 0, candidate ? slot.record.frame_id : 0, &call);
    const auto configuration_result = configured;
    const auto configuration_sdk_result = self->status.sdk_result;
    // Expired CPU tokens and incomplete/duplicate sources disable interpolation
    // while retaining ordinary WSI presentation for the application's frame.
    const bool valid = candidate && configured == RSF_BACKEND_OK;
    if (configured != RSF_BACKEND_OK) {
        self->status.presentation.result = configured;
        configured = invoke(*self, disable);
        if (configured != RSF_BACKEND_OK) return VK_ERROR_INITIALIZATION_FAILED;
    }
    // SDK proxy operations may wait for their CPU presenter. Its real WSI
    // operations take the LL2 broker internally, not around this outer call.
    const auto result = self->present(static_cast<VkQueue>(self->setup.queues[0]), &info);
    if (valid && (result == VK_SUCCESS || result == VK_SUBOPTIMAL_KHR)) {
        const auto device = static_cast<VkDevice>(self->setup.graphics.device);
        if (self->reset_fences(device, 1, &slot.consumer_completion) != VK_SUCCESS || self->setup.graphics_submit(self->setup.queue_user,
            0, nullptr, reinterpret_cast<uint64_t>(slot.consumer_completion)) != VK_SUCCESS) slot.completion_unknown = true;
        else slot.consumer_pending = true;
    }
    const auto state = invoke(*self, [](void*, uintptr_t, void* user) -> rsf_backend_result { return query_state(*static_cast<rsf_dlssgvk*>(user)); });
    if (valid && (result == VK_SUCCESS || result == VK_SUBOPTIMAL_KHR)) {
        slot.sdk_inputs_pending = true;
        slot.sdk_inputs_completion = state == RSF_BACKEND_OK ? self->last_inputs_completion : VK_NULL_HANDLE;
        slot.sdk_inputs_value = state == RSF_BACKEND_OK ? self->last_inputs_value : 0;
    }
    if (configuration_result != RSF_BACKEND_OK) self->status.sdk_result = configuration_sdk_result;
    self->status.asynchronous_present_result = asynchronous_error.exchange(0);
    self->presented = self->latest; self->presented_sequence = slot.recording_sequence;
    self->status.presentation.source_frame = valid ? self->latest : 0;
    self->status.presentation.enabled = valid && !self->status.sdk_status && !self->status.asynchronous_present_result;
    self->status.presentation.result = result == VK_SUCCESS || result == VK_SUBOPTIMAL_KHR ?
        (configuration_result != RSF_BACKEND_OK ? configuration_result : state) : RSF_BACKEND_ERROR_FEATURE_FAILED;
    if ((valid && self->status.sdk_status) || self->status.asynchronous_present_result)
        self->status.presentation.result = RSF_BACKEND_ERROR_FEATURE_FAILED;
    return result;
#else
    (void)self; (void)present_info; return -3;
#endif
} catch (...) { return -3; }
extern "C" rsf_backend_result rsf_dlssgvk_get_status(rsf_dlssgvk* self, rsf_dlssgvk_status* status) {
    if (!self || !status || status->struct_size < sizeof(*status)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (status->abi_version != RSF_DLSSGVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if defined(RSF_HAVE_STREAMLINE_VK)
    *status = self->status; return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
extern "C" rsf_backend_result rsf_dlssgvk_drain(rsf_dlssgvk* self) try {
    if (!self) return RSF_BACKEND_OK;
#if defined(RSF_HAVE_STREAMLINE_VK)
    const auto timeout_epoch = VulkanStorySlWorkerTimeoutEpoch();
    const auto disabled = invoke(*self, disable); if (disabled != RSF_BACKEND_OK) return disabled;
    QueueLock lock(*self);
    if (self->idle(static_cast<VkDevice>(self->setup.graphics.device)) != VK_SUCCESS ||
        timeout_epoch != VulkanStorySlWorkerTimeoutEpoch()) return RSF_BACKEND_ERROR_NOT_READY;
    for (auto& slot : self->slots) if (wait_inputs(*self, slot) != RSF_BACKEND_OK) return RSF_BACKEND_ERROR_NOT_READY;
    (void)self->capture.flush();
    self->present_capture.flush();
    return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" rsf_backend_result rsf_dlssgvk_destroy(rsf_dlssgvk* self) try {
    if (!self) return RSF_BACKEND_OK;
#if defined(RSF_HAVE_STREAMLINE_VK)
    auto result = rsf_dlssgvk_drain(self); if (result != RSF_BACKEND_OK) return result;
    result = invoke(*self, release_resources); if (result != RSF_BACKEND_OK) return result;
    result = self->present_capture.shutdown(); if (result != RSF_BACKEND_OK) return result;
    if (self->chain) {
        self->destroy_chain(static_cast<VkDevice>(self->setup.graphics.device), self->chain,
            static_cast<const VkAllocationCallbacks*>(self->setup.allocator));
        self->chain = VK_NULL_HANDLE;
    }
#endif
    delete self; return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
