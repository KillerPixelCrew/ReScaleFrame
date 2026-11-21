// SPDX-License-Identifier: MIT
#pragma once
#include <cstdint>
#include <vulkan/vulkan.h>

struct rsf_vkfg_barrier {
    uint64_t image;
    uint32_t old_layout, new_layout, source_family, target_family;
    uint32_t source_access, target_access, source_stages, target_stages;
};

struct rsf_vkfg_image_copy {
    uint64_t source, target;
    uint32_t source_layout, target_layout, width, height;
};

static_assert(sizeof(rsf_vkfg_barrier) == 40 && sizeof(rsf_vkfg_image_copy) == 32);
static_assert(VK_IMAGE_LAYOUT_UNDEFINED == 0 && VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL == 5 &&
    VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL == 6 && VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL == 7);
static_assert(VK_QUEUE_FAMILY_IGNORED == ~0u && VK_ACCESS_MEMORY_READ_BIT == 0x8000 &&
    VK_ACCESS_MEMORY_WRITE_BIT == 0x10000 && VK_PIPELINE_STAGE_ALL_COMMANDS_BIT == 0x10000);

namespace rsf::vkfg {
inline void record_barrier(PFN_vkCmdPipelineBarrier barrier, VkCommandBuffer commands, const rsf_vkfg_barrier& plan) noexcept {
    VkImageMemoryBarrier data{}; data.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
    data.oldLayout = static_cast<VkImageLayout>(plan.old_layout);
    data.newLayout = static_cast<VkImageLayout>(plan.new_layout);
    data.srcAccessMask = plan.source_access; data.dstAccessMask = plan.target_access;
    data.srcQueueFamilyIndex = plan.source_family; data.dstQueueFamilyIndex = plan.target_family;
    data.image = reinterpret_cast<VkImage>(plan.image);
    data.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    barrier(commands, plan.source_stages, plan.target_stages, 0, 0, nullptr, 0, nullptr, 1, &data);
}

inline void record_copy(PFN_vkCmdCopyImage copy, VkCommandBuffer commands, const rsf_vkfg_image_copy& plan) noexcept {
    VkImageCopy region{};
    region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    region.extent = {plan.width, plan.height, 1};
    copy(commands, reinterpret_cast<VkImage>(plan.source), static_cast<VkImageLayout>(plan.source_layout),
        reinterpret_cast<VkImage>(plan.target), static_cast<VkImageLayout>(plan.target_layout), 1, &region);
}
}
