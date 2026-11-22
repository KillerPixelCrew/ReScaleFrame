/* SPDX-License-Identifier: MIT */
#pragma once
#include "../../runtime/vulkan/fsr3/wsi-dispatch.h"

extern "C" {
VkResult rsf_fixture_sdk_wsi_create(VkDevice, VkSwapchainKHR*);
void rsf_fixture_sdk_wsi_destroy(VkDevice, VkSwapchainKHR);
VkResult rsf_fixture_sdk_wsi_present(VkDevice, VkQueue);
VkResult rsf_fixture_sdk_wsi_acquire(VkDevice, VkSwapchainKHR);
VkResult rsf_fixture_sdk_wsi_images(VkDevice, VkSwapchainKHR);
int32_t rsfFfxSetVulkanWsiDispatch(const rsf_ffx_wsi_dispatch*);
int32_t rsf_ffx_vulkan_wsi_dispatch_fixture();
}
