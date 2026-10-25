// SPDX-License-Identifier: GPL-3.0-only
#include "native_dlss_vulkan.h"
#include <rescaleframe/native_streamline_vulkan.h>
#include <atomic>
#include <cmath>
#include <cstring>
#include <memory>
#include <limits>

#if defined(RSF_HAVE_STREAMLINE_VK)
#define VK_NO_PROTOTYPES
#include <windows.h>
#include <vulkan/vulkan.h>
#include <sl.h>
#include <sl_dlss.h>
#include <DirectXMath.h>
#include "../../backends/common/frame_capture_vk.h"
namespace {
std::atomic<bool> occupied{false};
const sl::ViewportHandle viewport(1);
struct Image { VkImage image = VK_NULL_HANDLE; VkDeviceMemory memory = VK_NULL_HANDLE; VkImageView view = VK_NULL_HANDLE; bool initialized = false; };
sl::DLSSMode mode(uint32_t quality) {
    constexpr sl::DLSSMode modes[] = {sl::DLSSMode::eDLAA, sl::DLSSMode::eMaxQuality, sl::DLSSMode::eBalanced,
        sl::DLSSMode::eMaxPerformance, sl::DLSSMode::eUltraPerformance};
    return modes[quality];
}
sl::float4x4 matrix(const float* values) {
    sl::float4x4 result{};
    for (uint32_t row = 0; row < 4; ++row) for (uint32_t column = 0; column < 4; ++column)
        (&result[row].x)[column] = values[row * 4 + column];
    return result;
}
template<class T> T entry(void* module, const char* name) { return reinterpret_cast<T>(GetProcAddress(static_cast<HMODULE>(module), name)); }
}
struct rsf_dlssvk {
    rsf_srvk_setup setup{};
    uint32_t width = 0, height = 0, preset = 0;
    uint64_t session = 0, frame = 0; uint32_t view = 0, generation = 0;
    bool claimed = false, resources = false;
    Image color, depth, motion, output;
    VkPhysicalDeviceMemoryProperties memory{};
    PFN_vkCreateImage create_image = nullptr; PFN_vkDestroyImage destroy_image = nullptr;
    PFN_vkGetImageMemoryRequirements requirements = nullptr; PFN_vkAllocateMemory allocate = nullptr;
    PFN_vkFreeMemory free_memory = nullptr; PFN_vkBindImageMemory bind = nullptr;
    PFN_vkCreateImageView create_view = nullptr; PFN_vkDestroyImageView destroy_view = nullptr;
    PFN_vkCmdPipelineBarrier barrier = nullptr; PFN_vkCmdCopyImage copy = nullptr;
    PFun_slSetConstants* constants = nullptr; PFun_slSetTagForFrame* tags = nullptr;
    PFun_slEvaluateFeature* evaluate = nullptr; PFun_slFreeResources* release = nullptr;
    PFun_slDLSSGetOptimalSettings* optimal = nullptr; PFun_slDLSSSetOptions* options = nullptr;
    rsf::FrameCaptureVk capture;
    ~rsf_dlssvk() {
        const auto device = static_cast<VkDevice>(setup.device);
        for (Image* image : {&color, &depth, &motion, &output}) {
            if (image->view) destroy_view(device, image->view, nullptr);
            if (image->image) destroy_image(device, image->image, nullptr);
            if (image->memory) free_memory(device, image->memory, nullptr);
        }
        if (claimed) occupied = false;
    }
};
namespace {
bool allocate(rsf_dlssvk& self, Image& image, uint32_t width, uint32_t height, VkFormat format, VkImageUsageFlags usage) {
    VkImageCreateInfo info{}; info.sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO; info.imageType = VK_IMAGE_TYPE_2D;
    info.extent = {width, height, 1}; info.mipLevels = info.arrayLayers = 1; info.format = format;
    info.samples = VK_SAMPLE_COUNT_1_BIT; info.tiling = VK_IMAGE_TILING_OPTIMAL; info.usage = usage;
    const auto device = static_cast<VkDevice>(self.setup.device);
    if (self.create_image(device, &info, nullptr, &image.image) != VK_SUCCESS) return false;
    VkMemoryRequirements requirements{}; self.requirements(device, image.image, &requirements);
    uint32_t type = UINT32_MAX;
    for (uint32_t i = 0; i < self.memory.memoryTypeCount; ++i)
        if ((requirements.memoryTypeBits & (1u << i)) && (self.memory.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT)) { type = i; break; }
    if (type == UINT32_MAX) return false;
    VkMemoryAllocateInfo allocation{}; allocation.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO;
    allocation.allocationSize = requirements.size; allocation.memoryTypeIndex = type;
    if (self.allocate(device, &allocation, nullptr, &image.memory) != VK_SUCCESS || self.bind(device, image.image, image.memory, 0) != VK_SUCCESS) return false;
    VkImageViewCreateInfo view{}; view.sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
    view.image = image.image; view.viewType = VK_IMAGE_VIEW_TYPE_2D; view.format = format;
    view.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    return self.create_view(device, &view, nullptr, &image.view) == VK_SUCCESS;
}
void transition(rsf_dlssvk& self, VkCommandBuffer commands, VkImage image, VkImageLayout before, VkImageLayout after) {
    VkImageMemoryBarrier barrier{}; barrier.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
    barrier.image = image; barrier.oldLayout = before; barrier.newLayout = after;
    barrier.srcAccessMask = before == VK_IMAGE_LAYOUT_UNDEFINED ? 0 : VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    barrier.dstAccessMask = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    barrier.srcQueueFamilyIndex = barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    self.barrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &barrier);
}
void stage(rsf_dlssvk& self, VkCommandBuffer commands, const rsf_srvk_image& source, Image& destination) {
    const auto image = reinterpret_cast<VkImage>(source.image);
    transition(self, commands, image, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL);
    transition(self, commands, destination.image, destination.initialized ? VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL : VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
    VkImageCopy region{}; region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1}; region.extent = {source.width, source.height, 1};
    self.copy(commands, image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, destination.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
    transition(self, commands, image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
    transition(self, commands, destination.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
    destination.initialized = true;
}
sl::Resource resource(const Image& image, uint32_t width, uint32_t height, VkFormat format, bool output = false) {
    sl::Resource result{sl::ResourceType::eTex2d, reinterpret_cast<void*>(image.image), nullptr,
        reinterpret_cast<void*>(image.view), uint32_t(output ? VK_IMAGE_LAYOUT_GENERAL : VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL)};
    result.width = width; result.height = height; result.nativeFormat = format; result.mipLevels = result.arrayLayers = 1;
    result.flags = 0;
    result.usage = output ? VK_IMAGE_USAGE_STORAGE_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT : VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    return result;
}
rsf_backend_result initialize(void* module, uintptr_t, void* user) {
    auto& self = *static_cast<rsf_dlssvk*>(user);
    auto supported = entry<PFun_slIsFeatureSupported*>(module, "slIsFeatureSupported");
    auto feature = entry<PFun_slGetFeatureFunction*>(module, "slGetFeatureFunction");
    self.constants = entry<PFun_slSetConstants*>(module, "slSetConstants"); self.tags = entry<PFun_slSetTagForFrame*>(module, "slSetTagForFrame");
    self.evaluate = entry<PFun_slEvaluateFeature*>(module, "slEvaluateFeature"); self.release = entry<PFun_slFreeResources*>(module, "slFreeResources");
    if (!supported || !feature || !self.constants || !self.tags || !self.evaluate || !self.release) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    sl::AdapterInfo adapter{}; adapter.vkPhysicalDevice = self.setup.physical_device;
    if (supported(sl::kFeatureDLSS, adapter) != sl::Result::eOk) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    void* optimal = nullptr; void* options = nullptr;
    if (feature(sl::kFeatureDLSS, "slDLSSGetOptimalSettings", optimal) != sl::Result::eOk ||
        feature(sl::kFeatureDLSS, "slDLSSSetOptions", options) != sl::Result::eOk || !optimal || !options) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    self.optimal = reinterpret_cast<PFun_slDLSSGetOptimalSettings*>(optimal); self.options = reinterpret_cast<PFun_slDLSSSetOptions*>(options);
    sl::DLSSOptions request{}; request.mode = mode(self.setup.quality);
    request.outputWidth = self.setup.output_width; request.outputHeight = self.setup.output_height;
    sl::DLSSOptimalSettings plan{};
    if (self.optimal(request, plan) != sl::Result::eOk || !plan.optimalRenderWidth || !plan.optimalRenderHeight) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    self.width = plan.optimalRenderWidth; self.height = plan.optimalRenderHeight;
    return RSF_BACKEND_OK;
}
struct Evaluation { rsf_dlssvk& self; VkCommandBuffer commands; const rsf_srvk_frame& input; };
rsf_backend_result release_effect(void*, uintptr_t, void* user) {
    auto& self = *static_cast<rsf_dlssvk*>(user);
    if (self.release(sl::kFeatureDLSS, viewport) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self.resources = false;
    return RSF_BACKEND_OK;
}
rsf_backend_result evaluate(void*, uintptr_t token, void* user) {
    auto& call = *static_cast<Evaluation*>(user); auto& self = call.self; const auto& input = call.input;
    const auto& record = *input.record; const auto& camera = record.camera;
    if (!token || !input.color.copy_source || !input.depth.copy_source || !input.motion.copy_source || input.depth.format != VK_FORMAT_R32_SFLOAT)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    for (const auto* values : {camera.view_to_clip, camera.clip_to_view, camera.clip_to_previous_clip, camera.view_to_world})
        for (uint32_t i = 0; i < 16; ++i) if (!std::isfinite(values[i])) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    DirectX::XMFLOAT4X4 previous{}, inverse{}; std::memcpy(&previous, camera.clip_to_previous_clip, sizeof(previous));
    DirectX::XMVECTOR determinant{};
    DirectX::XMStoreFloat4x4(&inverse, DirectX::XMMatrixInverse(&determinant, DirectX::XMLoadFloat4x4(&previous)));
    const float det = DirectX::XMVectorGetX(determinant);
    if (!std::isfinite(det) || std::abs(det) < 1e-12f) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const bool reset = (record.flags & RSF_FRAME_FLAG_RESET) || self.frame + 1 != record.frame_id || self.session != record.session_id ||
        self.view != record.view_id || self.generation != record.resource_generation;
    sl::Constants constants{}; constants.cameraViewToClip = matrix(camera.view_to_clip); constants.clipToCameraView = matrix(camera.clip_to_view);
    constants.clipToPrevClip = matrix(camera.clip_to_previous_clip); constants.prevClipToClip = matrix(&inverse.m[0][0]);
    constants.cameraPos = {camera.view_to_world[12], camera.view_to_world[13], camera.view_to_world[14]};
    constants.cameraRight = {camera.view_to_world[0], camera.view_to_world[1], camera.view_to_world[2]};
    constants.cameraUp = {camera.view_to_world[4], camera.view_to_world[5], camera.view_to_world[6]};
    constants.cameraFwd = {-camera.view_to_world[8], -camera.view_to_world[9], -camera.view_to_world[10]};
    constants.cameraNear = camera.near_plane; constants.cameraFar = camera.far_plane > 0 ? camera.far_plane : std::numeric_limits<float>::max();
    constants.cameraFOV = camera.vertical_fov_radians; constants.cameraAspectRatio = float(record.output_width) / record.output_height;
    constants.jitterOffset = {camera.jitter_pixels[0], camera.jitter_pixels[1]}; constants.cameraPinholeOffset = {0, 0};
    constants.mvecScale = {input.motion_scale_x / record.render_width, input.motion_scale_y / record.render_height};
    constants.depthInverted = camera.depth_inverted ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    constants.cameraMotionIncluded = sl::Boolean::eTrue; constants.motionVectors3D = sl::Boolean::eFalse;
    constants.motionVectorsJittered = camera.motion_jittered ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    constants.motionVectorsInvalidValue = std::numeric_limits<float>::max(); constants.reset = reset ? sl::Boolean::eTrue : sl::Boolean::eFalse;
    auto& frame = *reinterpret_cast<sl::FrameToken*>(token);
    if (self.constants(constants, frame, viewport) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    sl::DLSSOptions options{}; options.mode = mode(self.setup.quality); options.outputWidth = record.output_width; options.outputHeight = record.output_height;
    options.colorBuffersHDR = sl::Boolean::eTrue; options.useAutoExposure = sl::Boolean::eTrue; options.preExposure = input.pre_exposure;
    const auto preset = static_cast<sl::DLSSPreset>(self.preset);
    options.dlaaPreset = options.qualityPreset = options.balancedPreset = options.performancePreset = options.ultraPerformancePreset = options.ultraQualityPreset = preset;
    if (self.options(viewport, options) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    stage(self, call.commands, input.color, self.color); stage(self, call.commands, input.depth, self.depth); stage(self, call.commands, input.motion, self.motion);
    if (!self.output.initialized) {
        transition(self, call.commands, self.output.image, VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_GENERAL); self.output.initialized = true;
    }
    sl::Resource color = resource(self.color, self.width, self.height, VK_FORMAT_R16G16B16A16_SFLOAT);
    sl::Resource depth = resource(self.depth, self.width, self.height, VK_FORMAT_R32_SFLOAT);
    sl::Resource motion = resource(self.motion, self.width, self.height, VK_FORMAT_R16G16_SFLOAT);
    sl::Resource output = resource(self.output, record.output_width, record.output_height, VK_FORMAT_R16G16B16A16_SFLOAT, true);
    const sl::Extent render{0, 0, self.width, self.height}, presented{0, 0, record.output_width, record.output_height};
    sl::ResourceTag tags[] = {{&color, sl::kBufferTypeScalingInputColor, sl::ResourceLifecycle::eValidUntilEvaluate, &render},
        {&depth, sl::kBufferTypeDepth, sl::ResourceLifecycle::eValidUntilEvaluate, &render},
        {&motion, sl::kBufferTypeMotionVectors, sl::ResourceLifecycle::eValidUntilEvaluate, &render},
        {&output, sl::kBufferTypeScalingOutputColor, sl::ResourceLifecycle::eValidUntilEvaluate, &presented}};
    if (self.tags(frame, viewport, tags, 4, reinterpret_cast<sl::CommandBuffer*>(call.commands)) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    const sl::BaseStructure* inputs[] = {&viewport}; self.resources = true;
    if (self.evaluate(sl::kFeatureDLSS, frame, inputs, 1, reinterpret_cast<sl::CommandBuffer*>(call.commands)) != sl::Result::eOk) { self.frame = 0; return RSF_BACKEND_ERROR_FEATURE_FAILED; }
    const rsf_srvk_image out{self.setup.device, reinterpret_cast<uint64_t>(self.output.image), VK_FORMAT_R16G16B16A16_SFLOAT,
        record.output_width, record.output_height, VK_IMAGE_LAYOUT_GENERAL, 1};
    (void)self.capture.record(call.commands, input.color, record, "sr-linear-color", reset);
    (void)self.capture.record(call.commands, input.depth, record, "sr-depth", reset);
    (void)self.capture.record(call.commands, input.motion, record, "sr-motion-uv", reset);
    (void)self.capture.record(call.commands, out, record, "sr-output-linear", reset);
    transition(self, call.commands, self.output.image, VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL);
    VkImageCopy region{}; region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1}; region.extent = {record.output_width, record.output_height, 1};
    self.copy(call.commands, self.output.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, reinterpret_cast<VkImage>(input.output.image), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
    transition(self, call.commands, self.output.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_GENERAL);
    self.frame = record.frame_id; self.session = record.session_id; self.view = record.view_id; self.generation = record.resource_generation;
    return RSF_BACKEND_OK;
}
}
#else
struct rsf_dlssvk {};
#endif

rsf_backend_result rsf_dlssvk_create(const rsf_srvk_setup* setup, rsf_dlssvk** out) {
    *out = nullptr;
#if defined(RSF_HAVE_STREAMLINE_VK)
    auto self = std::make_unique<rsf_dlssvk>(); self->setup = *setup;
    if (occupied.exchange(true)) return RSF_BACKEND_ERROR_NOT_READY;
    self->claimed = true;
    const rsf_slvk_call call{sizeof(call), RSF_SLVK_ABI_VERSION, 0, 0, initialize, self.get()};
    const auto result = rsf_slvk_invoke(&call); if (result != RSF_BACKEND_OK) return result;
#define RSF_LOAD(member, function) self->member = reinterpret_cast<PFN_##function>(setup->device_proc(setup->device, #function)); if (!self->member) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT
    RSF_LOAD(create_image, vkCreateImage); RSF_LOAD(destroy_image, vkDestroyImage); RSF_LOAD(requirements, vkGetImageMemoryRequirements);
    RSF_LOAD(allocate, vkAllocateMemory); RSF_LOAD(free_memory, vkFreeMemory); RSF_LOAD(bind, vkBindImageMemory);
    RSF_LOAD(create_view, vkCreateImageView); RSF_LOAD(destroy_view, vkDestroyImageView); RSF_LOAD(barrier, vkCmdPipelineBarrier); RSF_LOAD(copy, vkCmdCopyImage);
#undef RSF_LOAD
    auto properties = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(setup->instance_proc(setup->instance, "vkGetPhysicalDeviceMemoryProperties"));
    if (!properties) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    properties(static_cast<VkPhysicalDevice>(setup->physical_device), &self->memory);
    self->capture.set_provider(0, RSF_SR_DLSS);
    if (!self->capture.initialize(static_cast<VkDevice>(setup->device), setup->device_proc, self->memory)) return RSF_BACKEND_ERROR_INIT_FAILED;
    constexpr auto usage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    if (!allocate(*self, self->color, self->width, self->height, VK_FORMAT_R16G16B16A16_SFLOAT, usage) ||
        !allocate(*self, self->depth, self->width, self->height, VK_FORMAT_R32_SFLOAT, usage) ||
        !allocate(*self, self->motion, self->width, self->height, VK_FORMAT_R16G16_SFLOAT, usage) ||
        !allocate(*self, self->output, setup->output_width, setup->output_height, VK_FORMAT_R16G16B16A16_SFLOAT, VK_IMAGE_USAGE_STORAGE_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT))
        return RSF_BACKEND_ERROR_INIT_FAILED;
    *out = self.release(); return RSF_BACKEND_OK;
#else
    (void)setup; return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
rsf_backend_result rsf_dlssvk_plan(const rsf_dlssvk* self, uint32_t* width, uint32_t* height) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    *width = self->width; *height = self->height; return RSF_BACKEND_OK;
#else
    (void)self; (void)width; (void)height; return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
rsf_backend_result rsf_dlssvk_evaluate(rsf_dlssvk* self, void* commands, const rsf_srvk_frame* input) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    Evaluation evaluation{*self, static_cast<VkCommandBuffer>(commands), *input};
    const rsf_slvk_call call{sizeof(call), RSF_SLVK_ABI_VERSION, input->record->session_id, input->record->frame_id, evaluate, &evaluation};
    return rsf_slvk_invoke(&call);
#else
    (void)self; (void)commands; (void)input; return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
rsf_backend_result rsf_dlssvk_preset(rsf_dlssvk* self, uint32_t preset) {
    if (!(preset == 0 || preset == 5 || preset == 6 || (preset >= 10 && preset <= 13))) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (self->preset != preset) {
        // The owner has drained this context. Retire the old NGX feature now;
        // changing options in place otherwise queues a plugin-owned lambda
        // whose three-Present delay can outlive a camera-free SDK shutdown.
        if (self->resources) {
            const rsf_slvk_call call{sizeof(call), RSF_SLVK_ABI_VERSION, 0, 0, release_effect, self};
            const auto result = rsf_slvk_invoke(&call); if (result != RSF_BACKEND_OK) return result;
        }
        self->preset = preset; self->frame = 0;
    }
    return RSF_BACKEND_OK;
#else
    (void)self; return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
rsf_backend_result rsf_dlssvk_destroy(rsf_dlssvk* self) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (self && self->resources) {
        const rsf_slvk_call call{sizeof(call), RSF_SLVK_ABI_VERSION, 0, 0, release_effect, self};
        const auto result = rsf_slvk_invoke(&call); if (result != RSF_BACKEND_OK) return result;
        self->resources = false;
    }
    if (self) (void)self->capture.flush();
#endif
    delete self; return RSF_BACKEND_OK;
}
