// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_fg_vulkan.h>
#include <rescaleframe/native_latency_vulkan.h>
#include <vulkan/vulkan.h>

using rsf_vkfg_scope = int32_t (*)(rsf_vk_wsi_call, void*);
using rsf_vkfg_queue_present = int32_t (*)(rsf_vk_function, void*, const void*);
using rsf_vkfg_lookup = int32_t (*)(rsf_vk_get_proc, void*, const char*, rsf_vk_function*);

extern "C" {
int32_t rsf_core_vkfg_device_proc(rsf_vkfg_lookup, rsf_vk_get_proc, void*, const char*, rsf_vk_function*);
VkResult rsf_core_vkfg_graphics_submit(const rsf_fgvk_setup*, uint32_t, const VkSubmitInfo*, VkFence);
int32_t rsf_core_vkfg_drain(const rsf_fgvk_setup*, rsf_vk_wsi_call, void*, PFN_vkDeviceWaitIdle);
VkResult rsf_core_vkfg_wsi_create(rsf_vkfg_scope, PFN_vkCreateSwapchainKHR, VkDevice,
    const VkSwapchainCreateInfoKHR*, const VkAllocationCallbacks*, VkSwapchainKHR*);
void rsf_core_vkfg_wsi_destroy(rsf_vkfg_scope, PFN_vkDestroySwapchainKHR, VkDevice,
    VkSwapchainKHR, const VkAllocationCallbacks*);
VkResult rsf_core_vkfg_wsi_acquire(rsf_vkfg_scope, rsf_vkfg_scope, PFN_vkAcquireNextImageKHR,
    VkDevice, VkSwapchainKHR, uint64_t, VkSemaphore, VkFence, uint32_t*);
VkResult rsf_core_vkfg_wsi_images(rsf_vkfg_scope, rsf_vkfg_scope, PFN_vkGetSwapchainImagesKHR,
    VkDevice, VkSwapchainKHR, uint32_t*, VkImage*);
VkResult rsf_core_vkfg_wsi_present(rsf_vkfg_scope, rsf_vkfg_queue_present, PFN_vkQueuePresentKHR,
    VkQueue, const VkPresentInfoKHR*);
}
