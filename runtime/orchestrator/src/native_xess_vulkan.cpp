// SPDX-License-Identifier: GPL-3.0-only
#include "native_xess_vulkan.h"
#include <rescaleframe/native_xess_vulkan_requirements.h>
#include <cmath>
#include <memory>
#include <mutex>

#if defined(RSF_HAVE_XESS_VK)
#define VK_NO_PROTOTYPES
#include <windows.h>
#include <xess/xess_vk.h>
#include <filesystem>
#include <cstring>
#include "../../backends/common/frame_capture_vk.h"

namespace {
rsf_backend_result sdk_result(xess_result_t result, rsf_backend_result failure) {
    if (result == XESS_RESULT_SUCCESS) return RSF_BACKEND_OK;
    if (result == XESS_RESULT_ERROR_UNSUPPORTED_DEVICE || result == XESS_RESULT_ERROR_UNSUPPORTED_DRIVER ||
        result == XESS_RESULT_ERROR_UNSUPPORTED || result == XESS_RESULT_ERROR_NOT_IMPLEMENTED) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    return failure;
}
struct Library {
    HMODULE module = nullptr;
    decltype(&xessVKGetRequiredInstanceExtensions) instance = nullptr;
    decltype(&xessVKGetRequiredDeviceExtensions) device = nullptr;
    decltype(&xessVKGetRequiredDeviceFeatures) features = nullptr;
    decltype(&xessVKCreateContext) create = nullptr;
    decltype(&xessVKInit) initialize = nullptr;
    decltype(&xessVKExecute) execute = nullptr;
    decltype(&xessDestroyContext) destroy = nullptr;
    decltype(&xessGetOptimalInputResolution) resolution = nullptr;
    decltype(&xessSetVelocityScale) velocity = nullptr;
    decltype(&xessGetVersion) version = nullptr;
    ~Library() { if (module) FreeLibrary(module); }
};
std::mutex session_guard;
std::shared_ptr<Library> library;
struct Image {
    VkImage image = VK_NULL_HANDLE;
    VkDeviceMemory memory = VK_NULL_HANDLE;
    VkImageView view = VK_NULL_HANDLE;
    bool initialized = false;
};
xess_quality_settings_t quality(uint32_t setting) {
    constexpr xess_quality_settings_t settings[] = {XESS_QUALITY_SETTING_AA, XESS_QUALITY_SETTING_QUALITY,
        XESS_QUALITY_SETTING_BALANCED, XESS_QUALITY_SETTING_PERFORMANCE, XESS_QUALITY_SETTING_ULTRA_PERFORMANCE,
        XESS_QUALITY_SETTING_ULTRA_QUALITY};
    return settings[setting];
}
}
struct rsf_xessvk {
    rsf_srvk_setup setup{};
    std::shared_ptr<Library> sdk;
    xess_context_handle_t context = nullptr;
    uint32_t width = 0, height = 0;
    uint64_t session = 0, frame = 0;
    uint32_t view = 0, generation = 0;
    Image color, depth, motion, output;
    VkPhysicalDeviceMemoryProperties memory{};
    PFN_vkCreateImage create_image = nullptr; PFN_vkDestroyImage destroy_image = nullptr;
    PFN_vkGetImageMemoryRequirements requirements = nullptr; PFN_vkAllocateMemory allocate = nullptr;
    PFN_vkFreeMemory free_memory = nullptr; PFN_vkBindImageMemory bind = nullptr;
    PFN_vkCreateImageView create_view = nullptr; PFN_vkDestroyImageView destroy_view = nullptr;
    PFN_vkCmdPipelineBarrier barrier = nullptr; PFN_vkCmdCopyImage copy = nullptr;
    rsf::FrameCaptureVk capture;
    ~rsf_xessvk() {
        if (context) (void)sdk->destroy(context);
        const auto device = static_cast<VkDevice>(setup.device);
        for (Image* image : {&color, &depth, &motion, &output}) {
            if (image->view) destroy_view(device, image->view, nullptr);
            if (image->image) destroy_image(device, image->image, nullptr);
            if (image->memory) free_memory(device, image->memory, nullptr);
        }
    }
};
namespace {
bool allocate(rsf_xessvk& self, Image& image, uint32_t width, uint32_t height, VkFormat format, VkImageUsageFlags usage) {
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
void transition(rsf_xessvk& self, VkCommandBuffer commands, VkImage image, VkImageLayout before, VkImageLayout after) {
    VkImageMemoryBarrier barrier{}; barrier.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
    barrier.image = image; barrier.oldLayout = before; barrier.newLayout = after;
    barrier.srcAccessMask = before == VK_IMAGE_LAYOUT_UNDEFINED ? 0 : VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    barrier.dstAccessMask = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    barrier.srcQueueFamilyIndex = barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    self.barrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &barrier);
}
void stage(rsf_xessvk& self, VkCommandBuffer commands, const rsf_srvk_image& source, Image& destination) {
    const auto image = reinterpret_cast<VkImage>(source.image);
    transition(self, commands, image, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL);
    transition(self, commands, destination.image, destination.initialized ? VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL : VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
    VkImageCopy region{}; region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1}; region.extent = {source.width, source.height, 1};
    self.copy(commands, image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, destination.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
    transition(self, commands, image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
    transition(self, commands, destination.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
    destination.initialized = true;
}
xess_vk_image_view_info resource(const Image& image, uint32_t width, uint32_t height, VkFormat format) {
    xess_vk_image_view_info result{}; result.image = image.image; result.imageView = image.view;
    result.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    result.width = width; result.height = height; result.format = format;
    return result;
}
}
#else
struct rsf_xessvk {};
#endif

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_xessvk_initialize(const rsf_xessvk_setup* setup) try {
    if (!setup || setup->struct_size < sizeof(*setup)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_XESSVK_REQUIREMENTS_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (!setup->runtime_directory_utf8 || !*setup->runtime_directory_utf8) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_XESS_VK)
    std::lock_guard lock(session_guard);
    if (library) return RSF_BACKEND_ERROR_NOT_READY;
    std::u8string directory(std::strlen(setup->runtime_directory_utf8), u8'\0');
    std::memcpy(directory.data(), setup->runtime_directory_utf8, directory.size());
    const auto path = std::filesystem::path(directory) / L"libxess.dll";
    if (!path.is_absolute()) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto sdk = std::make_shared<Library>();
    sdk->module = LoadLibraryExW(path.c_str(), nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!sdk->module) return RSF_BACKEND_ERROR_LOAD_FAILED;
#define RSF_XESS_LOAD(member, name) sdk->member = reinterpret_cast<decltype(sdk->member)>(GetProcAddress(sdk->module, #name)); if (!sdk->member) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT
    RSF_XESS_LOAD(instance, xessVKGetRequiredInstanceExtensions); RSF_XESS_LOAD(device, xessVKGetRequiredDeviceExtensions);
    RSF_XESS_LOAD(features, xessVKGetRequiredDeviceFeatures); RSF_XESS_LOAD(create, xessVKCreateContext);
    RSF_XESS_LOAD(initialize, xessVKInit); RSF_XESS_LOAD(execute, xessVKExecute); RSF_XESS_LOAD(destroy, xessDestroyContext);
    RSF_XESS_LOAD(resolution, xessGetOptimalInputResolution); RSF_XESS_LOAD(velocity, xessSetVelocityScale); RSF_XESS_LOAD(version, xessGetVersion);
#undef RSF_XESS_LOAD
    // Godot retires host services before its Vulkan device. Keep SDK-owned
    // creation-chain storage mapped through that later teardown and process exit.
    HMODULE pinned = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        reinterpret_cast<LPCWSTR>(sdk->module), &pinned)) return RSF_BACKEND_ERROR_LOAD_FAILED;
    library = std::move(sdk);
    return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_xessvk_instance_requirements(uint32_t* count, const char* const** names, uint32_t* minimum_api) try {
    if (!count || !names || !minimum_api) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *count = 0; *names = nullptr; *minimum_api = 0;
#if defined(RSF_HAVE_XESS_VK)
    std::lock_guard lock(session_guard);
    return library ? sdk_result(library->instance(count, names, minimum_api), RSF_BACKEND_ERROR_INIT_FAILED) : RSF_BACKEND_ERROR_NOT_READY;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_xessvk_device_requirements(void* instance, void* physical,
    uint32_t* count, const char* const** names, void** features) try {
    if (!instance || !physical || !count || !names || !features) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *count = 0; *names = nullptr;
#if defined(RSF_HAVE_XESS_VK)
    std::lock_guard lock(session_guard);
    if (!library) return RSF_BACKEND_ERROR_NOT_READY;
    auto result = library->device(static_cast<VkInstance>(instance), static_cast<VkPhysicalDevice>(physical), count, names);
    if (result != XESS_RESULT_SUCCESS) return sdk_result(result, RSF_BACKEND_ERROR_INIT_FAILED);
    return sdk_result(library->features(static_cast<VkInstance>(instance), static_cast<VkPhysicalDevice>(physical), features), RSF_BACKEND_ERROR_INIT_FAILED);
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_xessvk_shutdown(void) try {
#if defined(RSF_HAVE_XESS_VK)
    std::lock_guard lock(session_guard);
    if (library && library.use_count() != 1) return RSF_BACKEND_ERROR_NOT_READY;
    library.reset();
#endif
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

rsf_backend_result rsf_xessvk_create(const rsf_srvk_setup* setup, rsf_xessvk** out) {
    if (!out) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    if (!setup || setup->struct_size < sizeof(*setup)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_SRVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (!setup->instance || !setup->physical_device || !setup->device || !setup->instance_proc || !setup->device_proc ||
        !setup->output_width || !setup->output_height || setup->quality > 5 || setup->backend != RSF_SR_XESS ||
        setup->depth_inverted > 1 || setup->motion_jittered > 1) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_XESS_VK)
    auto self = std::make_unique<rsf_xessvk>(); self->setup = *setup;
    { std::lock_guard lock(session_guard); self->sdk = library; }
    if (!self->sdk) return RSF_BACKEND_ERROR_NOT_READY;
    auto result = self->sdk->create(static_cast<VkInstance>(setup->instance), static_cast<VkPhysicalDevice>(setup->physical_device),
        static_cast<VkDevice>(setup->device), &self->context);
    if (result != XESS_RESULT_SUCCESS) return sdk_result(result, RSF_BACKEND_ERROR_INIT_FAILED);
    xess_vk_init_params_t init{}; init.outputResolution = {setup->output_width, setup->output_height}; init.qualitySetting = quality(setup->quality);
    init.initFlags = XESS_INIT_FLAG_ENABLE_AUTOEXPOSURE | (setup->depth_inverted ? XESS_INIT_FLAG_INVERTED_DEPTH : 0u) |
        (setup->motion_jittered ? XESS_INIT_FLAG_JITTERED_MV : 0u);
    result = self->sdk->initialize(self->context, &init);
    if (result != XESS_RESULT_SUCCESS) return sdk_result(result, RSF_BACKEND_ERROR_INIT_FAILED);
    xess_2d_t dimensions{}, minimum{}, maximum{};
    result = self->sdk->resolution(self->context, &init.outputResolution, init.qualitySetting, &dimensions, &minimum, &maximum);
    if (result != XESS_RESULT_SUCCESS || !dimensions.x || !dimensions.y ||
        dimensions.x < minimum.x || dimensions.y < minimum.y || dimensions.x > maximum.x || dimensions.y > maximum.y)
        return RSF_BACKEND_ERROR_INIT_FAILED;
    self->width = dimensions.x; self->height = dimensions.y;
#define RSF_XESS_VK_LOAD(member, function) self->member = reinterpret_cast<PFN_##function>(setup->device_proc(setup->device, #function)); if (!self->member) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT
    RSF_XESS_VK_LOAD(create_image, vkCreateImage); RSF_XESS_VK_LOAD(destroy_image, vkDestroyImage); RSF_XESS_VK_LOAD(requirements, vkGetImageMemoryRequirements);
    RSF_XESS_VK_LOAD(allocate, vkAllocateMemory); RSF_XESS_VK_LOAD(free_memory, vkFreeMemory); RSF_XESS_VK_LOAD(bind, vkBindImageMemory);
    RSF_XESS_VK_LOAD(create_view, vkCreateImageView); RSF_XESS_VK_LOAD(destroy_view, vkDestroyImageView); RSF_XESS_VK_LOAD(barrier, vkCmdPipelineBarrier); RSF_XESS_VK_LOAD(copy, vkCmdCopyImage);
#undef RSF_XESS_VK_LOAD
    auto properties = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(setup->instance_proc(setup->instance, "vkGetPhysicalDeviceMemoryProperties"));
    if (!properties) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    properties(static_cast<VkPhysicalDevice>(setup->physical_device), &self->memory);
    xess_version_t version{}; (void)self->sdk->version(&version);
    self->capture.set_provider((uint64_t(version.major) << 48) | (uint64_t(version.minor) << 32) |
        (uint64_t(version.patch) << 16) | version.reserved, RSF_SR_XESS);
    if (!self->capture.initialize(static_cast<VkDevice>(setup->device), setup->device_proc, self->memory)) return RSF_BACKEND_ERROR_INIT_FAILED;
    constexpr auto usage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    if (!allocate(*self, self->color, self->width, self->height, VK_FORMAT_R16G16B16A16_SFLOAT, usage) ||
        !allocate(*self, self->depth, self->width, self->height, VK_FORMAT_R32_SFLOAT, usage) ||
        !allocate(*self, self->motion, self->width, self->height, VK_FORMAT_R16G16_SFLOAT, usage) ||
        !allocate(*self, self->output, setup->output_width, setup->output_height, VK_FORMAT_R16G16B16A16_SFLOAT,
            VK_IMAGE_USAGE_STORAGE_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT)) return RSF_BACKEND_ERROR_INIT_FAILED;
    *out = self.release(); return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}

rsf_backend_result rsf_xessvk_plan(const rsf_xessvk* self, uint32_t* width, uint32_t* height) {
    if (!self || !width || !height) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_XESS_VK)
    *width = self->width; *height = self->height; return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}

rsf_backend_result rsf_xessvk_evaluate(rsf_xessvk* self, void* commands, const rsf_srvk_frame* input) {
    if (!self || !commands || !input || input->struct_size < sizeof(*input) || !input->record) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (input->abi_version != RSF_SRVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if defined(RSF_HAVE_XESS_VK)
    const auto& record = *input->record;
    if (record.struct_size < sizeof(record) || record.camera.struct_size < sizeof(record.camera)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (record.abi_version != RSF_GAME_FRAME_ABI_VERSION || record.camera.abi_version != RSF_GAME_FRAME_ABI_VERSION)
        return RSF_BACKEND_ERROR_ABI_MISMATCH;
    const auto& camera = record.camera;
    if (!record.session_id || !record.frame_id || record.render_width != self->width || record.render_height != self->height ||
        record.output_width != self->setup.output_width || record.output_height != self->setup.output_height ||
        camera.depth_inverted != self->setup.depth_inverted || camera.motion_jittered != self->setup.motion_jittered ||
        !std::isfinite(input->motion_scale_x) || !std::isfinite(input->motion_scale_y) ||
        !std::isfinite(input->pre_exposure) || input->pre_exposure <= 0 || !std::isfinite(1.f / input->pre_exposure) ||
        !std::isfinite(camera.jitter_pixels[0]) || !std::isfinite(camera.jitter_pixels[1]) ||
        std::abs(camera.jitter_pixels[0]) > .5f || std::abs(camera.jitter_pixels[1]) > .5f) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto valid = [&](const rsf_srvk_image& image, uint32_t width, uint32_t height, uint32_t format, uint32_t layout) {
        return image.device == self->setup.device && image.image && image.width == width && image.height == height && image.format == format && image.layout == layout;
    };
    if (!valid(input->color, self->width, self->height, VK_FORMAT_R16G16B16A16_SFLOAT, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL) ||
        !valid(input->depth, self->width, self->height, VK_FORMAT_R32_SFLOAT, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL) ||
        !valid(input->motion, self->width, self->height, VK_FORMAT_R16G16_SFLOAT, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL) ||
        !valid(input->output, record.output_width, record.output_height, VK_FORMAT_R16G16B16A16_SFLOAT, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL) ||
        !input->color.copy_source || !input->depth.copy_source || !input->motion.copy_source ||
        input->output.image == input->color.image || input->output.image == input->depth.image || input->output.image == input->motion.image ||
        input->color.image == input->depth.image || input->color.image == input->motion.image || input->depth.image == input->motion.image)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (self->sdk->velocity(self->context, input->motion_scale_x, input->motion_scale_y) != XESS_RESULT_SUCCESS) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    const auto list = static_cast<VkCommandBuffer>(commands);
    stage(*self, list, input->color, self->color); stage(*self, list, input->depth, self->depth); stage(*self, list, input->motion, self->motion);
    transition(*self, list, self->output.image, self->output.initialized ? VK_IMAGE_LAYOUT_GENERAL : VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_GENERAL);
    self->output.initialized = true;
    xess_vk_execute_params_t data{};
    data.colorTexture = resource(self->color, self->width, self->height, VK_FORMAT_R16G16B16A16_SFLOAT);
    data.depthTexture = resource(self->depth, self->width, self->height, VK_FORMAT_R32_SFLOAT);
    data.velocityTexture = resource(self->motion, self->width, self->height, VK_FORMAT_R16G16_SFLOAT);
    data.outputTexture = resource(self->output, record.output_width, record.output_height, VK_FORMAT_R16G16B16A16_SFLOAT);
    data.jitterOffsetX = camera.jitter_pixels[0]; data.jitterOffsetY = camera.jitter_pixels[1]; data.exposureScale = 1.f / input->pre_exposure;
    data.inputWidth = self->width; data.inputHeight = self->height;
    data.resetHistory = (record.flags & RSF_FRAME_FLAG_RESET) || self->frame + 1 != record.frame_id || self->session != record.session_id ||
        self->view != record.view_id || self->generation != record.resource_generation;
    if (self->sdk->execute(self->context, list, &data) != XESS_RESULT_SUCCESS) { self->frame = 0; return RSF_BACKEND_ERROR_FEATURE_FAILED; }
    const rsf_srvk_image output{self->setup.device, reinterpret_cast<uint64_t>(self->output.image), VK_FORMAT_R16G16B16A16_SFLOAT,
        record.output_width, record.output_height, VK_IMAGE_LAYOUT_GENERAL, 1};
    (void)self->capture.record(list, input->color, record, "sr-linear-color", data.resetHistory);
    (void)self->capture.record(list, input->depth, record, "sr-depth", data.resetHistory);
    (void)self->capture.record(list, input->motion, record, "sr-motion-uv", data.resetHistory);
    (void)self->capture.record(list, output, record, "sr-output-linear", data.resetHistory);
    transition(*self, list, self->output.image, VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL);
    VkImageCopy region{}; region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1}; region.extent = {record.output_width, record.output_height, 1};
    self->copy(list, self->output.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, reinterpret_cast<VkImage>(input->output.image), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
    transition(*self, list, self->output.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_GENERAL);
    self->frame = record.frame_id; self->session = record.session_id; self->view = record.view_id; self->generation = record.resource_generation;
    return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}

rsf_backend_result rsf_xessvk_destroy(rsf_xessvk* self) {
#if defined(RSF_HAVE_XESS_VK)
    if (self) {
        (void)self->capture.flush();
        if (self->context && self->sdk->destroy(self->context) != XESS_RESULT_SUCCESS) return RSF_BACKEND_ERROR_FEATURE_FAILED;
        self->context = nullptr;
    }
#endif
    delete self; return RSF_BACKEND_OK;
}
