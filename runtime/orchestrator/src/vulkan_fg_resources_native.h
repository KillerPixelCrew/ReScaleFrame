// SPDX-License-Identifier: MIT
#pragma once
#include "vulkan_fg_resources.h"
#include "native_vulkan_timing.h"
#include <rescaleframe/native_fg_vulkan.h>
#include <vulkan/vulkan.h>

namespace rsf::vkfg {
struct Device {
    VkDevice device{};
    uint32_t local_types{};
    int32_t (*graphics_submit)(void*, uint32_t, const void*, uint64_t){};
    void* queue_user{};
    const char* wait_label{};
    uint64_t wait_frame{};
    PFN_vkCreateImage create_image{};
    PFN_vkDestroyImage destroy_image{};
    PFN_vkGetImageMemoryRequirements requirements{};
    PFN_vkAllocateMemory allocate{};
    PFN_vkFreeMemory free_memory{};
    PFN_vkBindImageMemory bind{};
    PFN_vkCreateImageView create_view{};
    PFN_vkDestroyImageView destroy_view{};
    PFN_vkCreateFence create_fence{};
    PFN_vkDestroyFence destroy_fence{};
    PFN_vkResetFences reset_fences{};
    PFN_vkWaitForFences wait_fences{};

    bool load(const rsf_fgvk_setup& setup, bool with_view) {
        device = static_cast<VkDevice>(setup.graphics.device);
        graphics_submit = setup.graphics_submit;
        queue_user = setup.queue_user;
#define RSF_VKFG_LOAD(member, name) member = reinterpret_cast<PFN_##name>(setup.graphics.device_proc(device, #name)); if (!member) return false
        RSF_VKFG_LOAD(create_image, vkCreateImage);
        RSF_VKFG_LOAD(destroy_image, vkDestroyImage);
        RSF_VKFG_LOAD(requirements, vkGetImageMemoryRequirements);
        RSF_VKFG_LOAD(allocate, vkAllocateMemory);
        RSF_VKFG_LOAD(free_memory, vkFreeMemory);
        RSF_VKFG_LOAD(bind, vkBindImageMemory);
        if (with_view) {
            RSF_VKFG_LOAD(create_view, vkCreateImageView);
            RSF_VKFG_LOAD(destroy_view, vkDestroyImageView);
        }
        RSF_VKFG_LOAD(create_fence, vkCreateFence);
        RSF_VKFG_LOAD(destroy_fence, vkDestroyFence);
        RSF_VKFG_LOAD(reset_fences, vkResetFences);
        RSF_VKFG_LOAD(wait_fences, vkWaitForFences);
#undef RSF_VKFG_LOAD
        return true;
    }

    void memory_types(const VkPhysicalDeviceMemoryProperties& properties) {
        local_types = 0;
        for (uint32_t i = 0; i < properties.memoryTypeCount && i < VK_MAX_MEMORY_TYPES; ++i)
            if (properties.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT) local_types |= 1u << i;
    }
};

inline VkImage image_handle(const rsf_vkfg_image& image) { return reinterpret_cast<VkImage>(image.image); }

inline constexpr rsf_vkfg_platform platform{
    [](void* user) { return static_cast<Device*>(user)->local_types; },
    [](void* user, uint32_t width, uint32_t height, uint32_t format, uint64_t* out) -> int32_t {
        const auto& native = *static_cast<Device*>(user);
        VkImageCreateInfo info{VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
        info.imageType = VK_IMAGE_TYPE_2D;
        info.extent = {width, height, 1};
        info.mipLevels = info.arrayLayers = 1;
        info.format = static_cast<VkFormat>(format);
        info.samples = VK_SAMPLE_COUNT_1_BIT;
        info.tiling = VK_IMAGE_TILING_OPTIMAL;
        info.usage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
        VkImage image{};
        const auto result = native.create_image(native.device, &info, nullptr, &image);
        *out = reinterpret_cast<uint64_t>(image);
        return result;
    },
    [](void* user, uint64_t image, uint64_t* size, uint32_t* types) {
        const auto& native = *static_cast<Device*>(user);
        VkMemoryRequirements required{};
        native.requirements(native.device, reinterpret_cast<VkImage>(image), &required);
        *size = required.size;
        *types = required.memoryTypeBits;
    },
    [](void* user, uint64_t size, uint32_t type, uint64_t* out) -> int32_t {
        const auto& native = *static_cast<Device*>(user);
        VkMemoryAllocateInfo info{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO};
        info.allocationSize = size;
        info.memoryTypeIndex = type;
        VkDeviceMemory memory{};
        const auto result = native.allocate(native.device, &info, nullptr, &memory);
        *out = reinterpret_cast<uint64_t>(memory);
        return result;
    },
    [](void* user, uint64_t image, uint64_t memory) -> int32_t {
        const auto& native = *static_cast<Device*>(user);
        return native.bind(native.device, reinterpret_cast<VkImage>(image), reinterpret_cast<VkDeviceMemory>(memory), 0);
    },
    [](void* user, uint64_t image, uint32_t format, uint64_t* out) -> int32_t {
        const auto& native = *static_cast<Device*>(user);
        VkImageViewCreateInfo info{VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO};
        info.image = reinterpret_cast<VkImage>(image);
        info.viewType = VK_IMAGE_VIEW_TYPE_2D;
        info.format = static_cast<VkFormat>(format);
        info.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
        VkImageView view{};
        const auto result = native.create_view(native.device, &info, nullptr, &view);
        *out = reinterpret_cast<uint64_t>(view);
        return result;
    },
    [](void* user, uint64_t view) {
        const auto& native = *static_cast<Device*>(user);
        native.destroy_view(native.device, reinterpret_cast<VkImageView>(view), nullptr);
    },
    [](void* user, uint64_t image) {
        const auto& native = *static_cast<Device*>(user);
        native.destroy_image(native.device, reinterpret_cast<VkImage>(image), nullptr);
    },
    [](void* user, uint64_t memory) {
        const auto& native = *static_cast<Device*>(user);
        native.free_memory(native.device, reinterpret_cast<VkDeviceMemory>(memory), nullptr);
    },
    [](void* user, uint64_t* out) -> int32_t {
        const auto& native = *static_cast<Device*>(user);
        VkFenceCreateInfo info{VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
        VkFence fence{};
        const auto result = native.create_fence(native.device, &info, nullptr, &fence);
        *out = reinterpret_cast<uint64_t>(fence);
        return result;
    },
    [](void* user, uint64_t fence) {
        const auto& native = *static_cast<Device*>(user);
        native.destroy_fence(native.device, reinterpret_cast<VkFence>(fence), nullptr);
    },
    [](void* user, uint64_t fence) -> int32_t {
        const auto& native = *static_cast<Device*>(user);
        const auto handle = reinterpret_cast<VkFence>(fence);
        const VulkanTimingScope timing(native.wait_label, native.wait_frame);
        return native.wait_fences(native.device, 1, &handle, VK_TRUE, 15000000000ull);
    },
    [](void* user, uint64_t fence) -> int32_t {
        const auto& native = *static_cast<Device*>(user);
        const auto handle = reinterpret_cast<VkFence>(fence);
        return native.reset_fences(native.device, 1, &handle);
    },
    [](void* user, uint64_t fence) {
        const auto& native = *static_cast<Device*>(user);
        return native.graphics_submit(native.queue_user, 0, nullptr, fence);
    },
};
}
