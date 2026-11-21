// SPDX-License-Identifier: MIT
#include <rescaleframe/native_sr_vulkan.h>
#include "vulkan_fidelityfx_native.h"

namespace {
struct Platform;
int32_t sharpening_valid(const rsf_sharpening_options* options) noexcept
{
    return rsf::sharpening_settings::valid(*options);
}
}

extern "C" {
int32_t rsf_core_srvk_create(const Platform*, const rsf_srvk_setup*, rsf_srvk**);
int32_t rsf_core_srvk_plan(const rsf_srvk*, uint32_t*, uint32_t*);
int32_t rsf_core_srvk_evaluate(rsf_srvk*, void*, const rsf_srvk_frame*, int32_t (*)(const rsf_sharpening_options*));
void rsf_core_srvk_retire_commands(rsf_srvk*, void*);
int32_t rsf_core_srvk_destroy(rsf_srvk*);
int32_t rsf_core_srvk_dlss_preset(rsf_srvk*, uint32_t);
int32_t rsf_core_srvk_spatial_fallback(const Platform*, const rsf_srvk_fallback*);
}

#if defined(RSF_HAVE_FFX_VK)
#include "native_dlss_vulkan_platform.h"
#include "native_xess_vulkan.h"
#include "../../backends/common/frame_capture_vk.h"
#include "../../backends/common/sharpening.h"
#include "sharpening_vulkan.h"
#include <ffx_api/ffx_api.h>
#include <ffx_api/ffx_upscale.h>
#include <ffx_api/vk/ffx_api_vk.h>
#include <cstdio>
#include <cstring>
#include <memory>

namespace {
constexpr size_t max_versions = 32;

struct Version {
    uint64_t id;
    uint32_t major, minor, patch;
};
struct Blit {
    rsf_vk_function barrier, blit;
    uint32_t features;
};
struct FfxDispatch {
    const rsf_srvk_frame* frame;
    rsf_srvk_image reactive, composition, output;
    float far_plane, sharpness;
    uint32_t reset;
};
struct FfxNative;
struct FfxOps {
    rsf_vkffx_loader loader;
    int32_t (*open)(const rsf_srvk_setup*, void* module, FfxNative**);
    void (*close)(FfxNative*);
    int32_t (*initialize_graphics)(FfxNative*, const rsf_srvk_setup*);
    int32_t (*versions)(FfxNative*, Version*, uint64_t*);
    int32_t (*create_effect)(FfxNative*, const rsf_srvk_setup*, uint64_t, uint32_t);
    int32_t (*effective_version)(FfxNative*, uint64_t*);
    int32_t (*destroy_effect)(FfxNative*);
    int32_t (*plan)(FfxNative*, const rsf_srvk_setup*, uint32_t*, uint32_t*);
    int32_t (*allocate)(FfxNative*, rsf_srvk_image*, uint32_t, uint64_t*);
    void (*release)(FfxNative*, uint64_t, uint64_t);
    void (*initialize_mask)(FfxNative*, void*, uint64_t);
    void (*initialize_output)(FfxNative*, void*, uint64_t);
    int32_t (*dispatch)(FfxNative*, void*, const FfxDispatch*);
    void (*copy_output)(FfxNative*, void*, const rsf_srvk_image*, const rsf_srvk_image*);
    int32_t (*sharpen)(FfxNative*, const rsf_srvk_setup*, void*, const rsf_srvk_frame*, uint64_t);
    void (*clear_sharpen)(FfxNative*);
    void (*log)(const rsf_srvk_frame*, const char*);
    void (*capture_provider)(FfxNative*, uint64_t, uint32_t);
    void (*capture_record)(FfxNative*, void*, const rsf_srvk_image*, const rsf_frame_record*, const char*, uint32_t);
    int32_t (*capture_wants)(FfxNative*, uint64_t);
    void (*capture_parameters)(const rsf_srvk_frame*, uint32_t, uint32_t);
    void (*retire_commands)(FfxNative*, void*);
    void (*flush_capture)(FfxNative*);
};
struct Platform {
    const rsf_vkdlss_platform* dlss;
    const rsf::xess_vk::Ops* xess;
    FfxOps ffx;
    int32_t (*fallback_probe)(const rsf_srvk_fallback*, Blit*);
    int32_t (*fallback_record)(const rsf_srvk_fallback*, const Blit*);
    int32_t (*dump_begin)(const rsf_frame_record*, uint32_t);
    int32_t (*dump_wants)(const rsf_frame_record*);
    void (*dump_result)(const rsf_frame_record*, const char*, uint32_t, int32_t);
};
static_assert(sizeof(Platform) == 256 && sizeof(Blit) == 24 && sizeof(Version) == 24);
static_assert(sizeof(FfxDispatch) == 144 && offsetof(FfxDispatch, reset) == 136);
static_assert(VK_FORMAT_R8_UNORM == 9 && VK_FORMAT_R16G16_SFLOAT == 83 &&
    VK_FORMAT_R16G16B16A16_SFLOAT == 97 && VK_FORMAT_R32_SFLOAT == 100 && VK_FORMAT_D32_SFLOAT == 126);
static_assert(VK_IMAGE_LAYOUT_GENERAL == 1 && VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL == 5 &&
    VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL == 7);
static_assert(VK_IMAGE_USAGE_TRANSFER_SRC_BIT == 1 && VK_IMAGE_USAGE_TRANSFER_DST_BIT == 2 &&
    VK_IMAGE_USAGE_SAMPLED_BIT == 4 && VK_IMAGE_USAGE_STORAGE_BIT == 8);
static_assert((VK_FORMAT_FEATURE_BLIT_SRC_BIT | VK_FORMAT_FEATURE_BLIT_DST_BIT |
    VK_FORMAT_FEATURE_SAMPLED_IMAGE_FILTER_LINEAR_BIT) == (0x400 | 0x800 | 0x1000));
static_assert(FFX_UPSCALE_ENABLE_HIGH_DYNAMIC_RANGE == 1 && FFX_UPSCALE_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION == 4 &&
    FFX_UPSCALE_ENABLE_DEPTH_INVERTED == 8 && FFX_UPSCALE_ENABLE_DEPTH_INFINITE == 16 && FFX_UPSCALE_ENABLE_AUTO_EXPOSURE == 32);
static_assert(RSF_SHARPENING_VENDOR == 4);

extern "C" rsf_vk_function rsf_core_srvk_dispatch_lookup(void*, const char*,
    int32_t (*)(rsf_vk_get_proc, void*, const char*, rsf_vk_function*));

int32_t call_device_proc(rsf_vk_get_proc proc, void* device, const char* name, rsf_vk_function* result) noexcept try
{
    *result = proc(device, name);
    return 1;
}
catch (...) { return 0; }

PFN_vkVoidFunction VKAPI_PTR sdk_device_proc(VkDevice device, const char* name) noexcept
{
    return reinterpret_cast<PFN_vkVoidFunction>(rsf_core_srvk_dispatch_lookup(device, name, call_device_proc));
}

struct FfxNative {
    VkDevice device = VK_NULL_HANDLE;
    rsf_vkffx_exports sdk;
    ffxContext effect = nullptr;
    PFN_vkCreateImage create_image = nullptr;
    PFN_vkDestroyImage destroy_image = nullptr;
    PFN_vkGetImageMemoryRequirements requirements = nullptr;
    PFN_vkAllocateMemory allocate = nullptr;
    PFN_vkFreeMemory free_memory = nullptr;
    PFN_vkBindImageMemory bind = nullptr;
    PFN_vkCmdPipelineBarrier barrier = nullptr;
    PFN_vkCmdClearColorImage clear = nullptr;
    PFN_vkCmdCopyImage copy = nullptr;
    VkPhysicalDeviceMemoryProperties memory{};
    rsf::FrameCaptureVk capture{rsf::FrameCaptureVk::Completion::command_retirement};
    rsf::sharpening::Vulkan sharpen;

};

int32_t open(const rsf_srvk_setup*, void* module, FfxNative** out) noexcept try
{
    auto native = std::make_unique<FfxNative>();
    if (!native->sdk.resolve(static_cast<HMODULE>(module))) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    *out = native.release();
    return RSF_BACKEND_OK;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t initialize_graphics(FfxNative* native, const rsf_srvk_setup* setup) noexcept try
{
    native->device = static_cast<VkDevice>(setup->device);
#define RSF_VK_LOAD(member, function) \
    native->member = reinterpret_cast<PFN_##function>(sdk_device_proc(native->device, #function)); \
    if (!native->member) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT
    RSF_VK_LOAD(create_image, vkCreateImage);
    RSF_VK_LOAD(destroy_image, vkDestroyImage);
    RSF_VK_LOAD(requirements, vkGetImageMemoryRequirements);
    RSF_VK_LOAD(allocate, vkAllocateMemory);
    RSF_VK_LOAD(free_memory, vkFreeMemory);
    RSF_VK_LOAD(bind, vkBindImageMemory);
    RSF_VK_LOAD(barrier, vkCmdPipelineBarrier);
    RSF_VK_LOAD(clear, vkCmdClearColorImage);
    RSF_VK_LOAD(copy, vkCmdCopyImage);
#undef RSF_VK_LOAD
    auto properties = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(
        setup->instance_proc(setup->instance, "vkGetPhysicalDeviceMemoryProperties"));
    if (!properties) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    properties(static_cast<VkPhysicalDevice>(setup->physical_device), &native->memory);
    return native->capture.initialize(native->device, setup->device_proc, native->memory)
        ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t versions(FfxNative* native, Version* out, uint64_t* count) noexcept try
{
    uint64_t ids[max_versions]{};
    const char* names[max_versions]{};
    *count = max_versions;
    ffxQueryDescGetVersions query{};
    query.header.type = FFX_API_QUERY_DESC_TYPE_GET_VERSIONS;
    query.createDescType = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE;
    query.outputCount = count;
    query.versionIds = ids;
    query.versionNames = names;
    if (native->sdk.query(nullptr, &query.header) != FFX_API_RETURN_OK || *count > max_versions)
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    for (uint64_t index = 0; index < *count; ++index) {
        Version value{ids[index], 0, 0, 0};
        if (names[index] && std::sscanf(names[index], "%u.%u.%u", &value.major, &value.minor, &value.patch) == 3)
            out[index] = value;
    }
    return RSF_BACKEND_OK;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t create_effect(FfxNative* native, const rsf_srvk_setup* setup, uint64_t id, uint32_t flags) noexcept try
{
    ffxOverrideVersion version{};
    version.header.type = FFX_API_DESC_TYPE_OVERRIDE_VERSION;
    version.versionId = id;
    ffxCreateBackendVKDesc backend{};
    backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK;
    backend.header.pNext = &version.header;
    backend.vkDevice = native->device;
    backend.vkPhysicalDevice = static_cast<VkPhysicalDevice>(setup->physical_device);
    backend.vkDeviceProcAddr = sdk_device_proc;
    ffxCreateContextDescUpscale create{};
    create.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE;
    create.header.pNext = &backend.header;
    create.maxRenderSize = create.maxUpscaleSize = {setup->output_width, setup->output_height};
    create.flags = flags;
    return native->sdk.create(&native->effect, &create.header, nullptr) == FFX_API_RETURN_OK
        ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t effective_version(FfxNative* native, uint64_t* out) noexcept try
{
    ffxQueryGetProviderVersion query{};
    query.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
    if (native->sdk.query(&native->effect, &query.header) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    *out = query.versionId;
    return RSF_BACKEND_OK;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t destroy_effect(FfxNative* native) noexcept try
{
    if (!native->effect) return RSF_BACKEND_OK;
    if (native->sdk.destroy(&native->effect, nullptr) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    native->effect = nullptr;
    return RSF_BACKEND_OK;
}
catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

int32_t plan(FfxNative* native, const rsf_srvk_setup* setup, uint32_t* width, uint32_t* height) noexcept try
{
    ffxQueryDescUpscaleGetRenderResolutionFromQualityMode query{};
    query.header.type = FFX_API_QUERY_DESC_TYPE_UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE;
    query.displayWidth = setup->output_width;
    query.displayHeight = setup->output_height;
    query.qualityMode = setup->quality;
    query.pOutRenderWidth = width;
    query.pOutRenderHeight = height;
    return native->sdk.query(&native->effect, &query.header) == FFX_API_RETURN_OK
        ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_NOT_SUPPORTED;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t allocate(FfxNative* native, rsf_srvk_image* image, uint32_t usage, uint64_t* memory) noexcept try
{
    VkImageCreateInfo info{};
    info.sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO;
    info.imageType = VK_IMAGE_TYPE_2D;
    info.extent = {image->width, image->height, 1};
    info.mipLevels = info.arrayLayers = 1;
    info.format = static_cast<VkFormat>(image->format);
    info.samples = VK_SAMPLE_COUNT_1_BIT;
    info.tiling = VK_IMAGE_TILING_OPTIMAL;
    info.usage = usage;
    info.sharingMode = VK_SHARING_MODE_EXCLUSIVE;
    VkImage handle = VK_NULL_HANDLE;
    if (native->create_image(native->device, &info, nullptr, &handle) != VK_SUCCESS) return RSF_BACKEND_ERROR_INIT_FAILED;
    image->image = reinterpret_cast<uint64_t>(handle);
    VkMemoryRequirements requirements{};
    native->requirements(native->device, handle, &requirements);
    uint32_t type = UINT32_MAX;
    for (uint32_t i = 0; i < native->memory.memoryTypeCount; ++i) {
        if (!(requirements.memoryTypeBits & (1u << i))) continue;
        type = i;
        if (native->memory.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT) break;
    }
    if (type == UINT32_MAX) return RSF_BACKEND_ERROR_INIT_FAILED;
    VkMemoryAllocateInfo allocation{};
    allocation.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO;
    allocation.allocationSize = requirements.size;
    allocation.memoryTypeIndex = type;
    VkDeviceMemory allocated = VK_NULL_HANDLE;
    if (native->allocate(native->device, &allocation, nullptr, &allocated) != VK_SUCCESS) return RSF_BACKEND_ERROR_INIT_FAILED;
    *memory = reinterpret_cast<uint64_t>(allocated);
    return native->bind(native->device, handle, allocated, 0) == VK_SUCCESS ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

void release(FfxNative* native, uint64_t image, uint64_t memory) noexcept
{
    if (image) native->destroy_image(native->device, reinterpret_cast<VkImage>(image), nullptr);
    if (memory) native->free_memory(native->device, reinterpret_cast<VkDeviceMemory>(memory), nullptr);
}

VkImageMemoryBarrier image_barrier(VkImage image, VkImageLayout from, VkImageLayout to,
    VkAccessFlags source, VkAccessFlags target) noexcept
{
    VkImageMemoryBarrier barrier{};
    barrier.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
    barrier.oldLayout = from;
    barrier.newLayout = to;
    barrier.srcAccessMask = source;
    barrier.dstAccessMask = target;
    barrier.srcQueueFamilyIndex = barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.image = image;
    barrier.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    return barrier;
}

void transition(const FfxNative* native, VkCommandBuffer commands, VkImageMemoryBarrier barrier) noexcept
{
    native->barrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT,
        0, 0, nullptr, 0, nullptr, 1, &barrier);
}

void initialize_mask(FfxNative* native, void* commands, uint64_t image) noexcept
{
    const auto list = static_cast<VkCommandBuffer>(commands);
    const auto mask = reinterpret_cast<VkImage>(image);
    transition(native, list, image_barrier(mask, VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 0, VK_ACCESS_TRANSFER_WRITE_BIT));
    const VkClearColorValue zero{};
    const VkImageSubresourceRange range{VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    native->clear(list, mask, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, &zero, 1, &range);
    transition(native, list, image_barrier(mask, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
        VK_ACCESS_TRANSFER_WRITE_BIT, VK_ACCESS_SHADER_READ_BIT));
}

void initialize_output(FfxNative* native, void* commands, uint64_t image) noexcept
{
    transition(native, static_cast<VkCommandBuffer>(commands), image_barrier(reinterpret_cast<VkImage>(image),
        VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_GENERAL, 0, VK_ACCESS_SHADER_WRITE_BIT));
}

FfxApiResource resource(const rsf_srvk_image& image, bool output = false) noexcept
{
    FfxApiResource value{};
    value.resource = reinterpret_cast<void*>(static_cast<uintptr_t>(image.image));
    value.description.type = FFX_API_RESOURCE_TYPE_TEXTURE2D;
    value.description.width = image.width;
    value.description.height = image.height;
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

int32_t dispatch(FfxNative* native, void* commands, const FfxDispatch* call) noexcept try
{
    const auto& frame = *call->frame;
    const auto& record = *frame.record;
    ffxDispatchDescUpscale data{};
    data.header.type = FFX_API_DISPATCH_DESC_TYPE_UPSCALE;
    data.commandList = commands;
    data.color = resource(frame.color);
    data.depth = resource(frame.depth);
    data.motionVectors = resource(frame.motion);
    data.reactive = resource(call->reactive);
    data.transparencyAndComposition = resource(call->composition);
    data.output = resource(call->output, true);
    data.jitterOffset = {record.camera.jitter_pixels[0], record.camera.jitter_pixels[1]};
    data.motionVectorScale = {frame.motion_scale_x, frame.motion_scale_y};
    data.preExposure = frame.pre_exposure;
    data.renderSize = {record.render_width, record.render_height};
    data.upscaleSize = {record.output_width, record.output_height};
    data.frameTimeDelta = record.frame_time_ms;
    data.cameraNear = record.camera.near_plane;
    data.cameraFar = call->far_plane;
    data.cameraFovAngleVertical = record.camera.vertical_fov_radians;
    data.viewSpaceToMetersFactor = 1;
    rsf::ffx_sharpening(data, call->sharpness);
    data.reset = call->reset != 0;
    return native->sdk.dispatch(&native->effect, &data.header) == FFX_API_RETURN_OK
        ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

void copy_output(FfxNative* native, void* commands, const rsf_srvk_image* output, const rsf_srvk_image* target) noexcept
{
    const auto list = static_cast<VkCommandBuffer>(commands);
    const auto image = reinterpret_cast<VkImage>(output->image);
    transition(native, list, image_barrier(image, VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        VK_ACCESS_SHADER_WRITE_BIT, VK_ACCESS_TRANSFER_READ_BIT));
    VkImageCopy region{};
    region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    region.extent = {output->width, output->height, 1};
    native->copy(list, image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, reinterpret_cast<VkImage>(target->image),
        VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
    transition(native, list, image_barrier(image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_GENERAL,
        VK_ACCESS_TRANSFER_READ_BIT, VK_ACCESS_SHADER_WRITE_BIT));
}

int32_t fallback_probe(const rsf_srvk_fallback* call, Blit* out) noexcept try
{
    const auto properties = reinterpret_cast<PFN_vkGetPhysicalDeviceFormatProperties>(
        call->instance_proc(call->instance, "vkGetPhysicalDeviceFormatProperties"));
    out->barrier = call->device_proc(call->device, "vkCmdPipelineBarrier");
    out->blit = call->device_proc(call->device, "vkCmdBlitImage");
    if (!properties || !out->barrier || !out->blit) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    VkFormatProperties format{};
    properties(static_cast<VkPhysicalDevice>(call->physical_device), static_cast<VkFormat>(call->color.format), &format);
    out->features = format.optimalTilingFeatures;
    return RSF_BACKEND_OK;
}
catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

int32_t fallback_record(const rsf_srvk_fallback* call, const Blit* ops) noexcept try
{
    const auto& source = call->color;
    const auto& output = call->output;
    const auto list = static_cast<VkCommandBuffer>(call->command_buffer);
    const auto barrier = reinterpret_cast<PFN_vkCmdPipelineBarrier>(ops->barrier);
    const auto blit = reinterpret_cast<PFN_vkCmdBlitImage>(ops->blit);
    auto change = image_barrier(reinterpret_cast<VkImage>(source.image), VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
        VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_ACCESS_SHADER_READ_BIT, VK_ACCESS_TRANSFER_READ_BIT);
    barrier(list, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr, 0, nullptr, 1, &change);
    const auto target = image_barrier(reinterpret_cast<VkImage>(output.image), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_ACCESS_MEMORY_WRITE_BIT, VK_ACCESS_TRANSFER_WRITE_BIT);
    barrier(list, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr, 0, nullptr, 1, &target);
    VkImageBlit region{};
    region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    region.srcOffsets[1] = {static_cast<int32_t>(source.width), static_cast<int32_t>(source.height), 1};
    region.dstOffsets[1] = {static_cast<int32_t>(output.width), static_cast<int32_t>(output.height), 1};
    blit(list, change.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, target.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        1, &region, VK_FILTER_LINEAR);
    change = image_barrier(change.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
        VK_ACCESS_TRANSFER_READ_BIT, VK_ACCESS_SHADER_READ_BIT);
    barrier(list, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &change);
    return RSF_BACKEND_OK;
}
catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

constexpr Platform native{
    rsf::dlss_vk::platform,
    rsf::xess_vk::platform,
    {rsf_vkffx_system_loader, open, [](FfxNative* owner) { delete owner; }, initialize_graphics, versions, create_effect,
        effective_version, destroy_effect, plan, allocate, release, initialize_mask, initialize_output, dispatch, copy_output,
        [](FfxNative* owner, const rsf_srvk_setup* setup, void* commands, const rsf_srvk_frame* frame, uint64_t output) -> int32_t {
            return owner->sharpen.execute(*setup, commands, *frame, reinterpret_cast<VkImage>(output));
        },
        [](FfxNative* owner) { owner->sharpen.clear(); },
        [](const rsf_srvk_frame* frame, const char* message) noexcept {
            try {
                if (frame->log) frame->log(frame->log_user, message);
                else OutputDebugStringA(message);
            } catch (...) {}
        },
        [](FfxNative* owner, uint64_t version, uint32_t backend) { owner->capture.set_provider(version, backend); },
        [](FfxNative* owner, void* commands, const rsf_srvk_image* image,
            const rsf_frame_record* record, const char* role, uint32_t reset) {
            (void)owner->capture.record(static_cast<VkCommandBuffer>(commands), *image, *record, role, reset);
        },
        [](FfxNative* owner, uint64_t frame) -> int32_t { return owner->capture.wants(frame); },
        [](const rsf_srvk_frame* frame, uint32_t backend, uint32_t reset) {
            rsf::capture_sr_parameters(*frame->record, backend, frame->motion_scale_x, frame->motion_scale_y, frame->pre_exposure, reset);
        },
        [](FfxNative* owner, void* commands) {
            owner->sharpen.retire(commands);
            owner->capture.retire_commands(static_cast<VkCommandBuffer>(commands));
        },
        [](FfxNative* owner) { (void)owner->capture.flush(); }},
    fallback_probe, fallback_record, rsf_input_dump_begin_record, rsf_input_dump_wants, rsf_input_dump_result,
};
constexpr const Platform* platform = &native;
}
#else
namespace {
constexpr const Platform* platform = nullptr;
}
#endif

extern "C" {
rsf_backend_result rsf_srvk_create(const rsf_srvk_setup* setup, rsf_srvk** out)
{
    return rsf_core_srvk_create(platform, setup, out);
}
rsf_backend_result rsf_srvk_plan(const rsf_srvk* owner, uint32_t* width, uint32_t* height)
{
    return rsf_core_srvk_plan(owner, width, height);
}
rsf_backend_result rsf_srvk_evaluate(rsf_srvk* owner, void* commands, const rsf_srvk_frame* frame)
{
    return rsf_core_srvk_evaluate(owner, commands, frame, sharpening_valid);
}
void rsf_srvk_retire_commands(rsf_srvk* owner, void* commands)
{
    rsf_core_srvk_retire_commands(owner, commands);
}
rsf_backend_result rsf_srvk_destroy(rsf_srvk* owner)
{
    return rsf_core_srvk_destroy(owner);
}
rsf_backend_result rsf_srvk_dlss_preset(rsf_srvk* owner, uint32_t preset)
{
    return rsf_core_srvk_dlss_preset(owner, preset);
}
rsf_backend_result rsf_srvk_spatial_fallback(const rsf_srvk_fallback* call)
{
    return rsf_core_srvk_spatial_fallback(platform, call);
}
}
