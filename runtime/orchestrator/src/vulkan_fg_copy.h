// SPDX-License-Identifier: MIT
#pragma once
#include "vulkan_fg_copy_abi.h"
#include <vulkan/vulkan.h>

static_assert(sizeof(rsf_vkfg_barrier) == 40 && sizeof(rsf_vkfg_image_copy) == 32);
static_assert(VK_IMAGE_LAYOUT_UNDEFINED == 0 && VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL == 5 &&
    VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL == 6 && VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL == 7);
static_assert(VK_QUEUE_FAMILY_IGNORED == ~0u && VK_ACCESS_MEMORY_READ_BIT == 0x8000 &&
    VK_ACCESS_MEMORY_WRITE_BIT == 0x10000 && VK_PIPELINE_STAGE_ALL_COMMANDS_BIT == 0x10000);

extern "C" {
void rsf_core_vkfg_record_barrier(PFN_vkCmdPipelineBarrier, VkCommandBuffer, const rsf_vkfg_barrier*);
void rsf_core_vkfg_record_copy(PFN_vkCmdCopyImage, VkCommandBuffer, const rsf_vkfg_image_copy*);
}
