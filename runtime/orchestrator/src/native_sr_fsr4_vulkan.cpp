// SPDX-License-Identifier: MIT
#include <windows.h>
#include <vulkan/vulkan.h>
#include "native_fg_dx12_vulkan_platform.h"
#include "native_sr_fsr4_vulkan_platform.h"

struct ID3D12Resource;
extern "C" int VulkanStoryFsr4CreateSharedImage(rsf_fsr4_bridge*, uint32_t, uint32_t,
    uint32_t, uint32_t, HANDLE*, ID3D12Resource**);
extern "C" int VulkanStoryFsr4CreateSharedFence(rsf_fsr4_bridge*, HANDLE*);
extern "C" int VulkanStoryFsr4PrepareDestroy(rsf_fsr4_bridge*);
extern "C" int VulkanStoryFsr4DestroyChecked(rsf_fsr4_bridge*);
extern "C" int32_t rsf_core_fsr4sr_info(const rsf_fsr4_bridge*, rsf_fsr4_bridge_info*);
extern "C" int32_t rsf_core_fsr4sr_sharpness(rsf_fsr4_bridge*, float);
extern "C" void rsf_core_fsr4sr_release_image(void*);

static_assert(sizeof(rsf_fsr4_bridge_create) == 56 && offsetof(rsf_fsr4_bridge_create, output_width) == 32);
static_assert(sizeof(rsf_fsr4_bridge_frame) == 112 && offsetof(rsf_fsr4_bridge_frame, reset) == 92 &&
    offsetof(rsf_fsr4_bridge_frame, done_value) == 104);
static_assert(sizeof(rsf_fsr4_bridge_info) == sizeof(rsf_srvk_fsr4_provider_info) &&
    offsetof(rsf_fsr4_bridge_info, provider_version) == offsetof(rsf_srvk_fsr4_provider_info, provider_version));
static_assert(VK_FORMAT_R16G16B16A16_SFLOAT == 97 && VK_FORMAT_R32_SFLOAT == 100 && VK_FORMAT_R16G16_SFLOAT == 83);
static_assert(RSF_FSR4_BRIDGE_ABI_VERSION == 1 && VK_NOT_READY == 1 &&
    VK_ERROR_OUT_OF_HOST_MEMORY == -1 && VK_ERROR_INITIALIZATION_FAILED == -3 && VK_ERROR_UNKNOWN == -13);
static_assert(VK_IMAGE_LAYOUT_UNDEFINED == 0 && VK_IMAGE_LAYOUT_GENERAL == 1 &&
    VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL == 5 && VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL == 6 &&
    VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL == 7 && VK_IMAGE_USAGE_SAMPLED_BIT == 4 && VK_IMAGE_USAGE_STORAGE_BIT == 8);

namespace {
int32_t open(const rsf_srvk_setup* setup, void** out) noexcept try {
    const rsf_vk_function entries[5] = {
        setup->instance_proc(setup->instance, "vkGetPhysicalDeviceMemoryProperties"),
        setup->device_proc(setup->device, "vkCmdPipelineBarrier"),
        setup->device_proc(setup->device, "vkCmdCopyImage"),
        setup->device_proc(setup->device, "vkWaitSemaphores"),
        setup->device_proc(setup->device, "vkDeviceWaitIdle"),
    };
    return rsf_fsr4vk_graphics_open(setup, &entries, out);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

template<auto Call, class... Args>
int32_t checked(Args... args) noexcept try {
    return Call(args...);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

int32_t create_fence(void* bridge, void** handle) noexcept {
    return checked<VulkanStoryFsr4CreateSharedFence>(static_cast<rsf_fsr4_bridge*>(bridge), handle);
}

int32_t create_image(rsf_fsr4_bridge* bridge, uint32_t width, uint32_t height, uint32_t format,
    uint32_t writable, void** handle, void** resource) noexcept {
    ID3D12Resource* image{};
    const int32_t result = checked<VulkanStoryFsr4CreateSharedImage>(bridge, width, height, format, writable, handle, &image);
    *resource = image;
    return result;
}

int32_t split(const rsf_fsr4vk_handoff* handoff, void* commands, uint64_t timeline,
    uint64_t previous, uint64_t ready, uint64_t done, void** continuation) noexcept try {
    return handoff->split_record(handoff->user, commands, timeline, previous, ready, done, continuation);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

const rsf_fsr4vk_platform& platform() {
    static const rsf_fsr4vk_platform ops{
        rsf_dx12vk_platform_ops(), open, rsf_fsr4vk_graphics_close,
        checked<RsfFsr4BridgeCreate>, checked<RsfFsr4BridgePlan>, rsf_core_fsr4sr_info,
        create_fence, create_image, rsf_core_fsr4sr_release_image, rsf_fsr4vk_capture_initialize, rsf_fsr4vk_capture_begin,
        rsf_fsr4vk_capture_record, rsf_fsr4vk_capture_parameters, rsf_fsr4vk_capture_flush,
        rsf_fsr4vk_graphics_transition, rsf_fsr4vk_graphics_copy, split, nullptr,
        rsf_core_fsr4sr_sharpness, checked<RsfFsr4BridgeEvaluate>, rsf_fsr4vk_graphics_sharpen,
        rsf_fsr4vk_graphics_retire, rsf_fsr4vk_graphics_clear_sharpen, rsf_fsr4vk_graphics_wait, rsf_fsr4vk_graphics_idle,
        checked<VulkanStoryFsr4PrepareDestroy>, checked<VulkanStoryFsr4DestroyChecked>,
    };
    return ops;
}
}

int32_t rsf_fsr4vk_create(const rsf_srvk_setup* setup, const rsf_fsr4vk_handoff* handoff, rsf_fsr4vk** out) {
    return rsf_core_fsr4vk_create(&platform(), setup, handoff, out);
}

extern "C" int32_t rsf_core_fsr4vk_create_admitted(const rsf_fsr4vk_platform*, const rsf_srvk_setup*,
    const rsf_fsr4vk_handoff*, rsf_fsr4vk**);

extern "C" int32_t rsf_fsr4vk_create_admitted(const rsf_srvk_setup* setup, const rsf_fsr4vk_handoff* handoff, rsf_fsr4vk** out) {
    return rsf_core_fsr4vk_create_admitted(&platform(), setup, handoff, out);
}
