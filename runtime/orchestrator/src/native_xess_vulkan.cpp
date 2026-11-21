// SPDX-License-Identifier: MIT
// XeSS Vulkan SR leaves. Rust (vulkan_sr/xess.rs) owns SDK export lookup, the
// SDK session, context lifetime, admission, sizing, history, capture order and
// teardown. These functions bind typed SDK exports and record Vulkan commands.
#include "native_xess_vulkan.h"
#include "vulkan_sdk_native.h"
#include <rescaleframe/native_xess_vulkan_requirements.h>

extern "C" {
int32_t rsf_core_xessvk_initialize(const rsf::xess_vk::Ops*, const rsf_xessvk_setup*);
int32_t rsf_core_xessvk_instance_requirements(const rsf::xess_vk::Ops*, uint32_t*, const char* const**, uint32_t*);
int32_t rsf_core_xessvk_device_requirements(const rsf::xess_vk::Ops*, void*, void*, uint32_t*, const char* const**, void**);
int32_t rsf_core_xessvk_shutdown(const rsf::xess_vk::Ops*);
}

#if defined(RSF_HAVE_XESS_VK)
#define VK_NO_PROTOTYPES
#include <windows.h>
#include <xess/xess_vk.h>
#include <cstddef>
#include <cstring>
#include <memory>
#include "../../backends/common/frame_capture_vk.h"
#include "sharpening_vulkan.h"
#include "vulkan_fg_copy.h"
#include "vulkan_fg_resources_native.h"

namespace rsf::xess_vk {
struct Texture {
    uint64_t image, view;
    uint32_t width, height, format;
};
struct Init {
    uint32_t width, height, quality, flags;
};
struct Execute {
    Texture color, depth, motion, output;
    float jitter[2];
    float exposure_scale;
    uint32_t input_width, input_height, reset;
};
struct Library {
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
};
using Entries = void*[10];
struct Native {
    const Library* sdk = nullptr;
    xess_context_handle_t context = nullptr;
    VkDevice device = VK_NULL_HANDLE;
    rsf::vkfg::Device gpu;
    PFN_vkCmdPipelineBarrier barrier = nullptr;
    PFN_vkCmdCopyImage copy = nullptr;
    // The SR owner retires its readbacks with the command buffer that recorded them.
    rsf::FrameCaptureVk capture{rsf::FrameCaptureVk::Completion::command_retirement};
    rsf::sharpening::Vulkan sharpen;
};
struct Ops {
    rsf_vk_sdk_loader loader;
    int32_t (*allocate_table)(Library**);
    uint32_t (*pin)(void*);
    void (*drop_table)(Library*);
    int32_t (*instance_requirements)(Library*, uint32_t*, const char* const**, uint32_t*);
    int32_t (*device_extensions)(Library*, void*, void*, uint32_t*, const char* const**);
    int32_t (*device_features)(Library*, void*, void*, void**);
    int32_t (*version)(Library*, uint16_t*);
    int32_t (*open)(Library*, Native**);
    void (*close)(Native*);
    int32_t (*create_context)(Native*, const rsf_srvk_setup*);
    int32_t (*initialize)(Native*, const Init*);
    int32_t (*resolution)(Native*, const Init*, uint32_t*);
    int32_t (*destroy_context)(Native*);
    int32_t (*bind_device)(Native*, const rsf_srvk_setup*);
    void* (*image_user)(Native*);
    const rsf_vkfg_platform* images;
    int32_t (*velocity)(Native*, float, float);
    void (*barrier)(Native*, void*, const rsf_vkfg_barrier*);
    void (*copy)(Native*, void*, const rsf_vkfg_image_copy*);
    int32_t (*execute)(Native*, void*, const Execute*);
    int32_t (*sharpen)(Native*, const rsf_srvk_setup*, void*, const rsf_srvk_frame*, uint64_t);
    void (*clear_sharpen)(Native*);
    void (*log)(const rsf_srvk_frame*, const char*);
    void (*capture_provider)(Native*, uint64_t, uint32_t);
    void (*capture_record)(Native*, void*, const rsf_srvk_image*, const rsf_frame_record*, const char*, uint32_t);
    void (*capture_parameters)(const rsf_srvk_frame*, uint32_t, uint32_t);
    void (*retire_commands)(Native*, void*);
    void (*flush_capture)(Native*);
    void (*bind_entries)(Library*, const Entries*);
    void* (*lookup)(void*, const char*);
};
static_assert(sizeof(Ops) == 32 * sizeof(void*) && sizeof(rsf_xessvk_setup) == 16 && sizeof(Init) == 16);
static_assert(offsetof(Ops, barrier) == 19 * sizeof(void*) && offsetof(Ops, copy) == 20 * sizeof(void*) &&
    offsetof(Ops, execute) == 21 * sizeof(void*) && offsetof(Ops, sharpen) == 22 * sizeof(void*));
static_assert(offsetof(Ops, bind_entries) == 30 * sizeof(void*) && offsetof(Ops, lookup) == 31 * sizeof(void*));
static_assert(sizeof(Entries) == 10 * sizeof(void*));
static_assert(sizeof(Texture) == 32);
static_assert(sizeof(Execute) == 152 && offsetof(Execute, reset) == 148);
// Values mirrored by the Rust owner.
static_assert(XESS_RESULT_SUCCESS == 0 && XESS_RESULT_ERROR_UNSUPPORTED_DEVICE == -1 &&
    XESS_RESULT_ERROR_UNSUPPORTED_DRIVER == -2 && XESS_RESULT_ERROR_NOT_IMPLEMENTED == -7 &&
    XESS_RESULT_ERROR_UNSUPPORTED == -10);
static_assert(XESS_INIT_FLAG_INVERTED_DEPTH == 1 << 1 && XESS_INIT_FLAG_JITTERED_MV == 1 << 7 &&
    XESS_INIT_FLAG_ENABLE_AUTOEXPOSURE == 1 << 8);
static_assert(XESS_QUALITY_SETTING_AA == 106 && XESS_QUALITY_SETTING_QUALITY == 103 &&
    XESS_QUALITY_SETTING_BALANCED == 102 && XESS_QUALITY_SETTING_PERFORMANCE == 101 &&
    XESS_QUALITY_SETTING_ULTRA_PERFORMANCE == 100 && XESS_QUALITY_SETTING_ULTRA_QUALITY == 104);
static_assert(VK_FORMAT_R16G16_SFLOAT == 83 && VK_FORMAT_R16G16B16A16_SFLOAT == 97 && VK_FORMAT_R32_SFLOAT == 100);
static_assert(VK_IMAGE_LAYOUT_GENERAL == 1 && VK_IMAGE_USAGE_TRANSFER_SRC_BIT == 1 && VK_IMAGE_USAGE_TRANSFER_DST_BIT == 2 &&
    VK_IMAGE_USAGE_SAMPLED_BIT == 4 && VK_IMAGE_USAGE_STORAGE_BIT == 8);

namespace {
int32_t allocate_table(Library** out) noexcept try {
    *out = new Library{};
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

void bind_entries(Library* sdk, const Entries* entries) noexcept {
    const auto& [instance, device, features, create, initialize, execute, destroy, resolution, velocity, version] =
        *entries;
    sdk->instance = reinterpret_cast<decltype(sdk->instance)>(instance);
    sdk->device = reinterpret_cast<decltype(sdk->device)>(device);
    sdk->features = reinterpret_cast<decltype(sdk->features)>(features);
    sdk->create = reinterpret_cast<decltype(sdk->create)>(create);
    sdk->initialize = reinterpret_cast<decltype(sdk->initialize)>(initialize);
    sdk->execute = reinterpret_cast<decltype(sdk->execute)>(execute);
    sdk->destroy = reinterpret_cast<decltype(sdk->destroy)>(destroy);
    sdk->resolution = reinterpret_cast<decltype(sdk->resolution)>(resolution);
    sdk->velocity = reinterpret_cast<decltype(sdk->velocity)>(velocity);
    sdk->version = reinterpret_cast<decltype(sdk->version)>(version);
}

int32_t instance_requirements(Library* sdk, uint32_t* count, const char* const** names, uint32_t* minimum) noexcept {
    return sdk->instance(count, names, minimum);
}

int32_t device_extensions(Library* sdk, void* instance, void* physical, uint32_t* count, const char* const** names) noexcept {
    return sdk->device(static_cast<VkInstance>(instance), static_cast<VkPhysicalDevice>(physical), count, names);
}

int32_t device_features(Library* sdk, void* instance, void* physical, void** features) noexcept {
    return sdk->features(static_cast<VkInstance>(instance), static_cast<VkPhysicalDevice>(physical), features);
}

int32_t version(Library* sdk, uint16_t* out) noexcept {
    xess_version_t value{};
    const auto result = sdk->version(&value);
    out[0] = value.major; out[1] = value.minor; out[2] = value.patch; out[3] = value.reserved;
    return result;
}

int32_t open(Library* sdk, Native** out) noexcept try {
    auto native = std::make_unique<Native>();
    native->sdk = sdk;
    *out = native.release();
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t create_context(Native* native, const rsf_srvk_setup* setup) noexcept {
    native->device = static_cast<VkDevice>(setup->device);
    return native->sdk->create(static_cast<VkInstance>(setup->instance),
        static_cast<VkPhysicalDevice>(setup->physical_device), native->device, &native->context);
}

int32_t initialize(Native* native, const Init* call) noexcept {
    xess_vk_init_params_t init{};
    init.outputResolution = {call->width, call->height};
    init.qualitySetting = static_cast<xess_quality_settings_t>(call->quality);
    init.initFlags = call->flags;
    return native->sdk->initialize(native->context, &init);
}

int32_t resolution(Native* native, const Init* call, uint32_t* out) noexcept {
    const xess_2d_t output{call->width, call->height};
    xess_2d_t recommended{}, minimum{}, maximum{};
    const auto result = native->sdk->resolution(native->context, &output,
        static_cast<xess_quality_settings_t>(call->quality), &recommended, &minimum, &maximum);
    const uint32_t values[] = {recommended.x, recommended.y, minimum.x, minimum.y, maximum.x, maximum.y};
    std::memcpy(out, values, sizeof(values));
    return result;
}

int32_t destroy_context(Native* native) noexcept {
    if (!native->context) return XESS_RESULT_SUCCESS;
    const auto result = native->sdk->destroy(native->context);
    if (result == XESS_RESULT_SUCCESS) native->context = nullptr;
    return result;
}

int32_t bind_device(Native* native, const rsf_srvk_setup* setup) noexcept try {
    if (!native->gpu.load_images(*setup, true)) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
#define RSF_XESS_VK_LOAD(member, function) \
    native->member = reinterpret_cast<PFN_##function>(setup->device_proc(setup->device, #function)); \
    if (!native->member) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT
    RSF_XESS_VK_LOAD(barrier, vkCmdPipelineBarrier);
    RSF_XESS_VK_LOAD(copy, vkCmdCopyImage);
#undef RSF_XESS_VK_LOAD
    const auto properties = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(
        setup->instance_proc(setup->instance, "vkGetPhysicalDeviceMemoryProperties"));
    if (!properties) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    VkPhysicalDeviceMemoryProperties memory{};
    properties(static_cast<VkPhysicalDevice>(setup->physical_device), &memory);
    native->gpu.memory_types(memory);
    return native->capture.initialize(native->device, setup->device_proc, memory)
        ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

void record_barrier(Native* native, void* commands, const rsf_vkfg_barrier* plan) noexcept {
    rsf::vkfg::record_barrier(native->barrier, static_cast<VkCommandBuffer>(commands), *plan);
}

void record_copy(Native* native, void* commands, const rsf_vkfg_image_copy* plan) noexcept {
    rsf::vkfg::record_copy(native->copy, static_cast<VkCommandBuffer>(commands), *plan);
}

int32_t velocity(Native* native, float x, float y) noexcept {
    return native->sdk->velocity(native->context, x, y);
}

xess_vk_image_view_info texture_view(const Texture& texture) noexcept {
    xess_vk_image_view_info result{};
    result.image = reinterpret_cast<VkImage>(texture.image);
    result.imageView = reinterpret_cast<VkImageView>(texture.view);
    result.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    result.width = texture.width;
    result.height = texture.height;
    result.format = static_cast<VkFormat>(texture.format);
    return result;
}

int32_t execute(Native* native, void* commands, const Execute* call) noexcept {
    xess_vk_execute_params_t data{};
    data.colorTexture = texture_view(call->color);
    data.depthTexture = texture_view(call->depth);
    data.velocityTexture = texture_view(call->motion);
    data.outputTexture = texture_view(call->output);
    data.jitterOffsetX = call->jitter[0];
    data.jitterOffsetY = call->jitter[1];
    data.exposureScale = call->exposure_scale;
    data.inputWidth = call->input_width;
    data.inputHeight = call->input_height;
    data.resetHistory = call->reset;
    return native->sdk->execute(native->context, static_cast<VkCommandBuffer>(commands), &data);
}

int32_t sharpen(Native* native, const rsf_srvk_setup* setup, void* commands, const rsf_srvk_frame* frame, uint64_t output) noexcept try {
    return native->sharpen.execute(*setup, commands, *frame, reinterpret_cast<VkImage>(output));
} catch (...) { return 0; }

void log(const rsf_srvk_frame* frame, const char* message) noexcept try {
    if (frame->log) frame->log(frame->log_user, message);
    else OutputDebugStringA(message);
} catch (...) {}

void capture_record(Native* native, void* commands, const rsf_srvk_image* image,
    const rsf_frame_record* record, const char* role, uint32_t reset) noexcept {
    (void)native->capture.record(static_cast<VkCommandBuffer>(commands), *image, *record, role, reset);
}

void capture_parameters(const rsf_srvk_frame* frame, uint32_t backend, uint32_t reset) noexcept try {
    rsf::capture_sr_parameters(*frame->record, backend, frame->motion_scale_x, frame->motion_scale_y, frame->pre_exposure, reset);
} catch (...) {}

void retire_commands(Native* native, void* commands) noexcept {
    native->sharpen.retire(commands);
    native->capture.retire_commands(static_cast<VkCommandBuffer>(commands));
}
}

extern const Ops native_ops{
    rsf_vk_sdk_system_loader, allocate_table, rsf_core_vk_sdk_pin, [](Library* sdk) noexcept { delete sdk; },
    instance_requirements, device_extensions, device_features, version,
    open, [](Native* native) noexcept { delete native; }, create_context, initialize, resolution, destroy_context,
    bind_device, [](Native* native) -> void* { return &native->gpu; }, &rsf::vkfg::platform,
    velocity, record_barrier, record_copy, execute, sharpen,
    [](Native* native) noexcept { native->sharpen.clear(); }, log,
    [](Native* native, uint64_t provider, uint32_t backend) noexcept { native->capture.set_provider(provider, backend); },
    capture_record, capture_parameters, retire_commands,
    [](Native* native) noexcept { (void)native->capture.flush(); },
    bind_entries, nullptr,
};
}
#endif

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_xessvk_initialize(const rsf_xessvk_setup* setup) {
    return rsf_core_xessvk_initialize(rsf::xess_vk::platform, setup);
}

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_xessvk_instance_requirements(uint32_t* count, const char* const** names, uint32_t* minimum_api) {
    return rsf_core_xessvk_instance_requirements(rsf::xess_vk::platform, count, names, minimum_api);
}

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_xessvk_device_requirements(void* instance, void* physical,
    uint32_t* count, const char* const** names, void** features) {
    return rsf_core_xessvk_device_requirements(rsf::xess_vk::platform, instance, physical, count, names, features);
}

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_xessvk_shutdown(void) {
    return rsf_core_xessvk_shutdown(rsf::xess_vk::platform);
}
