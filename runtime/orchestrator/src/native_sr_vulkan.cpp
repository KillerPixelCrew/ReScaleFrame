// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_sr_vulkan.h>
#include "native_dlss_vulkan.h"
#include "native_xess_vulkan.h"
#include <cmath>
#include <memory>
#include <limits>

#if defined(RSF_HAVE_FFX_VK)
#include <windows.h>
#include <ffx_api/ffx_api.h>
#include <ffx_api/ffx_upscale.h>
#include <ffx_api/vk/ffx_api_vk.h>
#include "../../backends/fsr/src/ffx_version.h"
#include "../../backends/common/frame_capture_vk.h"
#include <filesystem>
#include <mutex>
#include <unordered_map>
#include <cstring>

namespace {
struct DeviceDispatch { rsf_vk_get_proc proc; uint32_t references; };
std::mutex dispatch_guard;
std::unordered_map<void*, DeviceDispatch> dispatches;
PFN_vkVoidFunction VKAPI_PTR sdk_device_proc(VkDevice device, const char* name) noexcept try {
    rsf_vk_get_proc proc = nullptr;
    {
        std::lock_guard lock(dispatch_guard);
        auto found = dispatches.find(device);
        if (found == dispatches.end()) return nullptr;
        proc = found->second.proc;
    }
    auto result = proc(device, name);
    // Godot enables core Vulkan 1.1+ functionality without every promoted KHR
    // extension name. Older FidelityFX queries the KHR spellings explicitly.
    if (!result && name) {
        struct Alias { const char* extension; const char* core; };
        constexpr Alias aliases[] = {
            {"vkGetBufferMemoryRequirements2KHR", "vkGetBufferMemoryRequirements2"},
            {"vkGetImageMemoryRequirements2KHR", "vkGetImageMemoryRequirements2"},
            {"vkBindBufferMemory2KHR", "vkBindBufferMemory2"},
            {"vkBindImageMemory2KHR", "vkBindImageMemory2"}};
        for (const auto& alias : aliases) if (std::strcmp(name, alias.extension) == 0) result = proc(device, alias.core);
    }
    return reinterpret_cast<PFN_vkVoidFunction>(result);
} catch (...) { return nullptr; }
struct Image {
    VkImage image = VK_NULL_HANDLE;
    VkDeviceMemory memory = VK_NULL_HANDLE;
    bool initialized = false;
};
}

struct rsf_srvk {
    rsf_dlssvk* dlss = nullptr;
    rsf_xessvk* xess = nullptr;
    rsf_srvk_setup setup{};
    HMODULE module = nullptr;
    ffxContext effect = nullptr;
    PfnFfxCreateContext create = nullptr;
    PfnFfxDestroyContext destroy = nullptr;
    PfnFfxQuery query = nullptr;
    PfnFfxDispatch dispatch = nullptr;
    PFN_vkCreateImage create_image = nullptr;
    PFN_vkDestroyImage destroy_image = nullptr;
    PFN_vkGetImageMemoryRequirements requirements = nullptr;
    PFN_vkAllocateMemory allocate = nullptr;
    PFN_vkFreeMemory free_memory = nullptr;
    PFN_vkBindImageMemory bind = nullptr;
    PFN_vkCmdPipelineBarrier barrier = nullptr;
    PFN_vkCmdClearColorImage clear = nullptr;
    PFN_vkCmdCopyImage copy = nullptr;
    VkPhysicalDeviceMemoryProperties memory_properties{};
    Image output, zero_mask;
    uint32_t width = 0, height = 0;
    uint64_t version = 0, last_frame = 0, last_session = 0;
    uint32_t last_view = 0, last_generation = 0;
    bool registered = false;
    rsf::FrameCaptureVk capture;
    ~rsf_srvk() {
        if (effect && destroy) (void)destroy(&effect, nullptr);
        for (Image* image : {&output, &zero_mask}) {
            if (image->image && destroy_image) destroy_image(static_cast<VkDevice>(setup.device), image->image, nullptr);
            if (image->memory && free_memory) free_memory(static_cast<VkDevice>(setup.device), image->memory, nullptr);
        }
        if (registered) {
            std::lock_guard lock(dispatch_guard);
            auto found = dispatches.find(setup.device);
            if (found != dispatches.end() && --found->second.references == 0) dispatches.erase(found);
        }
        if (module) FreeLibrary(module);
    }
};

namespace {
template<class T> T device_function(rsf_srvk& self, const char* name) {
    return reinterpret_cast<T>(sdk_device_proc(static_cast<VkDevice>(self.setup.device), name));
}
bool allocate_image(rsf_srvk& self, Image& image, uint32_t width, uint32_t height, VkFormat format, VkImageUsageFlags usage) {
    VkImageCreateInfo info{}; info.sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO;
    info.imageType = VK_IMAGE_TYPE_2D; info.extent = {width, height, 1}; info.mipLevels = info.arrayLayers = 1;
    info.format = format; info.samples = VK_SAMPLE_COUNT_1_BIT; info.tiling = VK_IMAGE_TILING_OPTIMAL;
    info.usage = usage; info.sharingMode = VK_SHARING_MODE_EXCLUSIVE;
    auto device = static_cast<VkDevice>(self.setup.device);
    if (self.create_image(device, &info, nullptr, &image.image) != VK_SUCCESS) return false;
    VkMemoryRequirements requirements{}; self.requirements(device, image.image, &requirements);
    uint32_t memory_type = UINT32_MAX;
    for (uint32_t i = 0; i < self.memory_properties.memoryTypeCount; ++i) {
        if (!(requirements.memoryTypeBits & (1u << i))) continue;
        memory_type = i;
        if (self.memory_properties.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT) break;
    }
    if (memory_type == UINT32_MAX) return false;
    VkMemoryAllocateInfo allocation{}; allocation.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO;
    allocation.allocationSize = requirements.size; allocation.memoryTypeIndex = memory_type;
    return self.allocate(device, &allocation, nullptr, &image.memory) == VK_SUCCESS &&
        self.bind(device, image.image, image.memory, 0) == VK_SUCCESS;
}
void transition(rsf_srvk& self, VkCommandBuffer commands, VkImage image, VkImageLayout from, VkImageLayout to,
    VkAccessFlags source, VkAccessFlags target) {
    VkImageMemoryBarrier barrier{}; barrier.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
    barrier.oldLayout = from; barrier.newLayout = to; barrier.srcAccessMask = source; barrier.dstAccessMask = target;
    barrier.srcQueueFamilyIndex = barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.image = image; barrier.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    self.barrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT,
        0, 0, nullptr, 0, nullptr, 1, &barrier);
}
FfxApiResource resource(const rsf_srvk_image& image, bool output = false) {
    FfxApiResource value{};
    value.resource = reinterpret_cast<void*>(static_cast<uintptr_t>(image.image));
    value.description.type = FFX_API_RESOURCE_TYPE_TEXTURE2D;
    value.description.width = image.width; value.description.height = image.height;
    value.description.depth = value.description.mipCount = 1;
    value.description.usage = output ? FFX_API_RESOURCE_USAGE_UAV : FFX_API_RESOURCE_USAGE_READ_ONLY;
    if (image.format == VK_FORMAT_D32_SFLOAT) value.description.usage = FFX_API_RESOURCE_USAGE_DEPTHTARGET;
    switch (image.format) {
    case VK_FORMAT_R16G16B16A16_SFLOAT: value.description.format = FFX_API_SURFACE_FORMAT_R16G16B16A16_FLOAT; break;
    case VK_FORMAT_R16G16_SFLOAT: value.description.format = FFX_API_SURFACE_FORMAT_R16G16_FLOAT; break;
    case VK_FORMAT_R32_SFLOAT:
    case VK_FORMAT_D32_SFLOAT: value.description.format = FFX_API_SURFACE_FORMAT_R32_FLOAT; break;
    case VK_FORMAT_R8_UNORM: value.description.format = FFX_API_SURFACE_FORMAT_R8_UNORM; break;
    default: break;
    }
    value.state = output ? FFX_API_RESOURCE_STATE_UNORDERED_ACCESS : FFX_API_RESOURCE_STATE_COMPUTE_READ;
    return value;
}
bool valid_image(const rsf_srvk& self, const rsf_srvk_image& image, uint32_t width, uint32_t height, uint32_t layout) {
    return image.device == self.setup.device && image.image && image.width == width && image.height == height && image.layout == layout;
}
}
#else
struct rsf_srvk {};
#endif

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_srvk_spatial_fallback(const rsf_srvk_fallback* call) try {
    if (!call || call->struct_size < sizeof(*call)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (call->abi_version != RSF_SRVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    const auto& source = call->color;
    const auto& output = call->output;
    if (!call->instance || !call->physical_device || !call->device || !call->instance_proc || !call->device_proc || !call->command_buffer ||
        source.device != call->device || output.device != call->device || !source.image || !output.image ||
        source.image == output.image || !source.copy_source || source.format != 97 || output.format != 97 ||
        source.layout != 5 || output.layout != 7 || !source.width || !source.height || !output.width || !output.height ||
        source.width > INT32_MAX || source.height > INT32_MAX || output.width > INT32_MAX || output.height > INT32_MAX)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_FFX_VK)
    auto properties = reinterpret_cast<PFN_vkGetPhysicalDeviceFormatProperties>(call->instance_proc(call->instance, "vkGetPhysicalDeviceFormatProperties"));
    auto barrier = reinterpret_cast<PFN_vkCmdPipelineBarrier>(call->device_proc(call->device, "vkCmdPipelineBarrier"));
    auto blit = reinterpret_cast<PFN_vkCmdBlitImage>(call->device_proc(call->device, "vkCmdBlitImage"));
    if (!properties || !barrier || !blit) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    VkFormatProperties format{};
    properties(static_cast<VkPhysicalDevice>(call->physical_device), VK_FORMAT_R16G16B16A16_SFLOAT, &format);
    constexpr auto required = VK_FORMAT_FEATURE_BLIT_SRC_BIT | VK_FORMAT_FEATURE_BLIT_DST_BIT | VK_FORMAT_FEATURE_SAMPLED_IMAGE_FILTER_LINEAR_BIT;
    if ((format.optimalTilingFeatures & required) != required) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    const auto commands = static_cast<VkCommandBuffer>(call->command_buffer);
    VkImageMemoryBarrier change{}; change.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
    change.image = reinterpret_cast<VkImage>(source.image);
    change.srcQueueFamilyIndex = change.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    change.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    change.oldLayout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL; change.newLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL;
    change.srcAccessMask = VK_ACCESS_SHADER_READ_BIT; change.dstAccessMask = VK_ACCESS_TRANSFER_READ_BIT;
    barrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr, 0, nullptr, 1, &change);
    auto target = change; target.image = reinterpret_cast<VkImage>(output.image);
    target.oldLayout = target.newLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL;
    target.srcAccessMask = VK_ACCESS_MEMORY_WRITE_BIT; target.dstAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT;
    barrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr, 0, nullptr, 1, &target);
    VkImageBlit region{};
    region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    region.srcOffsets[1] = {static_cast<int32_t>(source.width), static_cast<int32_t>(source.height), 1};
    region.dstOffsets[1] = {static_cast<int32_t>(output.width), static_cast<int32_t>(output.height), 1};
    blit(commands, reinterpret_cast<VkImage>(source.image), VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        reinterpret_cast<VkImage>(output.image), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region, VK_FILTER_LINEAR);
    change.oldLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL; change.newLayout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
    change.srcAccessMask = VK_ACCESS_TRANSFER_READ_BIT; change.dstAccessMask = VK_ACCESS_SHADER_READ_BIT;
    barrier(commands, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &change);
    return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_SUPPORTED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_srvk_create(const rsf_srvk_setup* setup, rsf_srvk** out) try {
    if (!out) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    if (!setup || setup->struct_size < sizeof(*setup)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_SRVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (!setup->instance || !setup->physical_device || !setup->device || !setup->device_proc || !setup->instance_proc ||
        !setup->runtime_directory_utf8 || !setup->output_width || !setup->output_height ||
        (setup->quality > 4 && (setup->backend != RSF_SR_XESS || setup->quality > 5)) ||
        setup->depth_inverted > 1 || setup->depth_infinite > 1 || setup->motion_jittered > 1) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->backend != RSF_SR_DLSS && setup->backend != RSF_SR_XESS && setup->backend != RSF_SR_FSR2 && setup->backend != RSF_SR_FSR3) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
#if defined(RSF_HAVE_FFX_VK)
    auto self = std::make_unique<rsf_srvk>(); self->setup = *setup;
    if (setup->backend == RSF_SR_XESS) {
        const auto result = rsf_xessvk_create(setup, &self->xess);
        if (result != RSF_BACKEND_OK) return result;
        (void)rsf_xessvk_plan(self->xess, &self->width, &self->height);
        *out = self.release(); return RSF_BACKEND_OK;
    }
    if (setup->backend == RSF_SR_DLSS) {
        const auto result = rsf_dlssvk_create(setup, &self->dlss);
        if (result != RSF_BACKEND_OK) return result;
        (void)rsf_dlssvk_plan(self->dlss, &self->width, &self->height);
        *out = self.release(); return RSF_BACKEND_OK;
    }
    std::u8string directory(std::strlen(setup->runtime_directory_utf8), u8'\0');
    std::memcpy(directory.data(), setup->runtime_directory_utf8, directory.size());
    const auto path = std::filesystem::path(directory) / L"amd_fidelityfx_vk.dll";
    if (!path.is_absolute()) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    self->module = LoadLibraryExW(path.c_str(), nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!self->module) return RSF_BACKEND_ERROR_LOAD_FAILED;
    self->create = reinterpret_cast<PfnFfxCreateContext>(GetProcAddress(self->module, "ffxCreateContext"));
    self->destroy = reinterpret_cast<PfnFfxDestroyContext>(GetProcAddress(self->module, "ffxDestroyContext"));
    self->query = reinterpret_cast<PfnFfxQuery>(GetProcAddress(self->module, "ffxQuery"));
    self->dispatch = reinterpret_cast<PfnFfxDispatch>(GetProcAddress(self->module, "ffxDispatch"));
    if (!self->create || !self->destroy || !self->query || !self->dispatch) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    {
        std::lock_guard lock(dispatch_guard);
        auto [found, inserted] = dispatches.try_emplace(setup->device, DeviceDispatch{setup->device_proc, 0});
        if (!inserted && found->second.proc != setup->device_proc) return RSF_BACKEND_ERROR_WRONG_API;
        ++found->second.references; self->registered = true;
    }
#define RSF_VK_LOAD(member, function) self->member = device_function<PFN_##function>(*self, #function); if (!self->member) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT
    RSF_VK_LOAD(create_image, vkCreateImage); RSF_VK_LOAD(destroy_image, vkDestroyImage);
    RSF_VK_LOAD(requirements, vkGetImageMemoryRequirements); RSF_VK_LOAD(allocate, vkAllocateMemory);
    RSF_VK_LOAD(free_memory, vkFreeMemory); RSF_VK_LOAD(bind, vkBindImageMemory);
    RSF_VK_LOAD(barrier, vkCmdPipelineBarrier); RSF_VK_LOAD(clear, vkCmdClearColorImage); RSF_VK_LOAD(copy, vkCmdCopyImage);
#undef RSF_VK_LOAD
    auto memory_properties = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(setup->instance_proc(setup->instance, "vkGetPhysicalDeviceMemoryProperties"));
    if (!memory_properties) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    memory_properties(static_cast<VkPhysicalDevice>(setup->physical_device), &self->memory_properties);
    if (!self->capture.initialize(static_cast<VkDevice>(setup->device), setup->device_proc, self->memory_properties)) return RSF_BACKEND_ERROR_INIT_FAILED;
    uint64_t count = 32, ids[32]{}; const char* names[32]{}; char version_name[128]{};
    ffxQueryDescGetVersions versions{}; versions.header.type = FFX_API_QUERY_DESC_TYPE_GET_VERSIONS;
    versions.createDescType = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE;
    versions.outputCount = &count; versions.versionIds = ids; versions.versionNames = names;
    if (self->query(nullptr, &versions.header) != FFX_API_RETURN_OK || count > 32 ||
        !rsf::select_ffx_version(ids, names, count, setup->backend, 0, self->version, version_name, sizeof(version_name)))
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    ffxOverrideVersion version{}; version.header.type = FFX_API_DESC_TYPE_OVERRIDE_VERSION; version.versionId = self->version;
    ffxCreateBackendVKDesc backend{}; backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK;
    backend.header.pNext = &version.header; backend.vkDevice = static_cast<VkDevice>(setup->device);
    backend.vkPhysicalDevice = static_cast<VkPhysicalDevice>(setup->physical_device); backend.vkDeviceProcAddr = sdk_device_proc;
    ffxCreateContextDescUpscale create{}; create.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE; create.header.pNext = &backend.header;
    create.maxRenderSize = create.maxUpscaleSize = {setup->output_width, setup->output_height};
    create.flags = FFX_UPSCALE_ENABLE_HIGH_DYNAMIC_RANGE | FFX_UPSCALE_ENABLE_AUTO_EXPOSURE |
        (setup->depth_inverted ? FFX_UPSCALE_ENABLE_DEPTH_INVERTED : 0u) |
        (setup->depth_infinite ? FFX_UPSCALE_ENABLE_DEPTH_INFINITE : 0u) |
        (setup->motion_jittered ? FFX_UPSCALE_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION : 0u);
    if (self->create(&self->effect, &create.header, nullptr) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_INIT_FAILED;
    ffxQueryGetProviderVersion effective{}; effective.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
    if (self->query(&self->effect, &effective.header) != FFX_API_RETURN_OK || effective.versionId != self->version)
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    self->capture.set_provider(self->version, setup->backend);
    if (setup->quality == 0 && setup->backend == RSF_SR_FSR2) {
        self->width = setup->output_width; self->height = setup->output_height;
    } else {
        ffxQueryDescUpscaleGetRenderResolutionFromQualityMode plan{};
        plan.header.type = FFX_API_QUERY_DESC_TYPE_UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE;
        plan.displayWidth = setup->output_width; plan.displayHeight = setup->output_height; plan.qualityMode = setup->quality;
        plan.pOutRenderWidth = &self->width; plan.pOutRenderHeight = &self->height;
        if (self->query(&self->effect, &plan.header) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    }
    if (!self->width || !self->height ||
        !allocate_image(*self, self->output, setup->output_width, setup->output_height, VK_FORMAT_R16G16B16A16_SFLOAT, VK_IMAGE_USAGE_STORAGE_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT) ||
        !allocate_image(*self, self->zero_mask, self->width, self->height, VK_FORMAT_R8_UNORM, VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT))
        return RSF_BACKEND_ERROR_INIT_FAILED;
    *out = self.release(); return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_srvk_plan(const rsf_srvk* self, uint32_t* width, uint32_t* height) {
    if (!self || !width || !height) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_FFX_VK)
    *width = self->width; *height = self->height; return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_srvk_evaluate(rsf_srvk* self, void* commands, const rsf_srvk_frame* input) try {
    if (!self || !commands || !input || input->struct_size < sizeof(*input)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (input->abi_version != RSF_SRVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if defined(RSF_HAVE_FFX_VK)
    const auto* record = input->record;
    if (!record || record->struct_size < sizeof(*record) || record->abi_version != RSF_GAME_FRAME_ABI_VERSION ||
        record->camera.struct_size < sizeof(record->camera) || record->camera.abi_version != RSF_GAME_FRAME_ABI_VERSION ||
        !record->session_id || !record->frame_id || record->render_width != self->width || record->render_height != self->height ||
        record->output_width != self->setup.output_width || record->output_height != self->setup.output_height ||
        record->camera.render_width != record->render_width || record->camera.render_height != record->render_height ||
        record->camera.output_width != record->output_width || record->camera.output_height != record->output_height ||
        record->camera.depth_inverted != self->setup.depth_inverted || record->camera.motion_jittered != self->setup.motion_jittered ||
        !std::isfinite(record->frame_time_ms) || record->frame_time_ms <= 0 ||
        !std::isfinite(input->motion_scale_x) || !std::isfinite(input->motion_scale_y) || !input->motion_scale_x || !input->motion_scale_y ||
        !std::isfinite(input->pre_exposure) || input->pre_exposure <= 0 ||
        !std::isfinite(record->camera.near_plane) || record->camera.near_plane <= 0 ||
        !std::isfinite(record->camera.far_plane) || record->camera.far_plane < 0 ||
        (!self->setup.depth_infinite && record->camera.far_plane <= record->camera.near_plane) ||
        !std::isfinite(record->camera.vertical_fov_radians) || record->camera.vertical_fov_radians <= 0 || record->camera.vertical_fov_radians >= 3.141593f ||
        !std::isfinite(record->camera.jitter_pixels[0]) || !std::isfinite(record->camera.jitter_pixels[1])) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!valid_image(*self, input->color, self->width, self->height, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL) || input->color.format != VK_FORMAT_R16G16B16A16_SFLOAT ||
        !valid_image(*self, input->depth, self->width, self->height, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL) || (input->depth.format != VK_FORMAT_D32_SFLOAT && input->depth.format != VK_FORMAT_R32_SFLOAT) ||
        !valid_image(*self, input->motion, self->width, self->height, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL) || input->motion.format != VK_FORMAT_R16G16_SFLOAT ||
        !valid_image(*self, input->output, self->setup.output_width, self->setup.output_height, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL) || input->output.format != VK_FORMAT_R16G16B16A16_SFLOAT)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    for (const auto* mask : {&input->reactive, &input->composition}) if (mask->image &&
        (!valid_image(*self, *mask, self->width, self->height, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL) || mask->format != VK_FORMAT_R8_UNORM))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    (void)rsf_input_dump_begin_record(record, self->setup.backend);
    if (rsf_input_dump_wants(record)) {
        auto capture_list = static_cast<VkCommandBuffer>(commands);
        (void)self->capture.record(capture_list, input->color, *record, "sr-source-jittered", record->flags & RSF_FRAME_FLAG_RESET);
        if (input->reactive.image) (void)self->capture.record(capture_list, input->reactive, *record, "sr-reactive-mask", 0);
        if (input->composition.image) (void)self->capture.record(capture_list, input->composition, *record, "sr-transparency-mask", 0);
    }
    if (self->dlss || self->xess) {
        const auto result = self->dlss ? rsf_dlssvk_evaluate(self->dlss, commands, input) : rsf_xessvk_evaluate(self->xess, commands, input);
        rsf_input_dump_result(record, "sr", self->setup.backend, result); return result;
    }
    auto list = static_cast<VkCommandBuffer>(commands);
    if (!self->zero_mask.initialized) {
        transition(*self, list, self->zero_mask.image, VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 0, VK_ACCESS_TRANSFER_WRITE_BIT);
        VkClearColorValue zero{}; VkImageSubresourceRange range{VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
        self->clear(list, self->zero_mask.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, &zero, 1, &range);
        transition(*self, list, self->zero_mask.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, VK_ACCESS_TRANSFER_WRITE_BIT, VK_ACCESS_SHADER_READ_BIT);
        self->zero_mask.initialized = true;
    }
    if (!self->output.initialized) {
        transition(*self, list, self->output.image, VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_GENERAL, 0, VK_ACCESS_SHADER_WRITE_BIT);
        self->output.initialized = true;
    }
    const rsf_srvk_image zero{self->setup.device, reinterpret_cast<uint64_t>(self->zero_mask.image), VK_FORMAT_R8_UNORM, self->width, self->height, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 0};
    const rsf_srvk_image output{self->setup.device, reinterpret_cast<uint64_t>(self->output.image), VK_FORMAT_R16G16B16A16_SFLOAT, self->setup.output_width, self->setup.output_height, VK_IMAGE_LAYOUT_GENERAL, 1};
    ffxDispatchDescUpscale data{}; data.header.type = FFX_API_DISPATCH_DESC_TYPE_UPSCALE;
    data.commandList = commands; data.color = resource(input->color); data.depth = resource(input->depth); data.motionVectors = resource(input->motion);
    data.reactive = resource(input->reactive.image ? input->reactive : zero);
    data.transparencyAndComposition = resource(input->composition.image ? input->composition : zero); data.output = resource(output, true);
    data.jitterOffset = {record->camera.jitter_pixels[0], record->camera.jitter_pixels[1]};
    data.motionVectorScale = {input->motion_scale_x, input->motion_scale_y}; data.preExposure = input->pre_exposure;
    data.renderSize = {self->width, self->height}; data.upscaleSize = {self->setup.output_width, self->setup.output_height};
    data.frameTimeDelta = record->frame_time_ms; data.cameraNear = record->camera.near_plane;
    data.cameraFar = record->camera.far_plane > 0 ? record->camera.far_plane : std::numeric_limits<float>::max();
    data.cameraFovAngleVertical = record->camera.vertical_fov_radians; data.viewSpaceToMetersFactor = 1;
    data.reset = (record->flags & RSF_FRAME_FLAG_RESET) || self->last_frame + 1 != record->frame_id || self->last_session != record->session_id ||
        self->last_view != record->view_id || self->last_generation != record->resource_generation;
    rsf::capture_sr_parameters(*record, self->setup.backend, input->motion_scale_x, input->motion_scale_y, input->pre_exposure, data.reset);
    if (self->capture.wants(record->frame_id)) {
        (void)self->capture.record(list, input->color, *record, "sr-linear-color", data.reset);
        (void)self->capture.record(list, input->depth, *record, "sr-depth", data.reset);
        (void)self->capture.record(list, input->motion, *record, "sr-motion-uv", data.reset);
    }
    if (self->dispatch(&self->effect, &data.header) != FFX_API_RETURN_OK) {
        self->last_frame = 0; rsf_input_dump_result(record, "sr", self->setup.backend, RSF_BACKEND_ERROR_FEATURE_FAILED); return RSF_BACKEND_ERROR_FEATURE_FAILED;
    }
    (void)self->capture.record(list, output, *record, "sr-output-linear", data.reset);
    transition(*self, list, self->output.image, VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_ACCESS_SHADER_WRITE_BIT, VK_ACCESS_TRANSFER_READ_BIT);
    VkImageCopy region{}; region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    region.extent = {self->setup.output_width, self->setup.output_height, 1};
    self->copy(list, self->output.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        reinterpret_cast<VkImage>(static_cast<uintptr_t>(input->output.image)), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
    transition(*self, list, self->output.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_GENERAL, VK_ACCESS_TRANSFER_READ_BIT, VK_ACCESS_SHADER_WRITE_BIT);
    self->last_frame = record->frame_id; self->last_session = record->session_id; self->last_view = record->view_id; self->last_generation = record->resource_generation;
    rsf_input_dump_result(record, "sr", self->setup.backend, RSF_BACKEND_OK);
    return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_srvk_destroy(rsf_srvk* self) try {
#if defined(RSF_HAVE_FFX_VK)
    if (self && self->dlss) {
        const auto result = rsf_dlssvk_destroy(self->dlss);
        if (result != RSF_BACKEND_OK) return result;
        self->dlss = nullptr;
    }
    if (self && self->xess) {
        const auto result = rsf_xessvk_destroy(self->xess);
        if (result != RSF_BACKEND_OK) return result;
        self->xess = nullptr;
    }
    if (self && self->effect) {
        (void)self->capture.flush();
        if (self->destroy(&self->effect, nullptr) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
        self->effect = nullptr;
    }
#endif
    delete self; return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_srvk_dlss_preset(rsf_srvk* self, uint32_t preset) try {
    if (!self) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_FFX_VK)
    return self->dlss ? rsf_dlssvk_preset(self->dlss, preset) : RSF_BACKEND_ERROR_NOT_SUPPORTED;
#else
    (void)preset; return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
