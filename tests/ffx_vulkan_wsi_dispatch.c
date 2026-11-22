/* SPDX-License-Identifier: MIT */
/* Compiles the corrected SDK's C11 WSI dispatch section and gives its static
   entries linkage for the Rust scenarios in vulkan-fg/src/wsi.rs. */
#define RSF_FFX_WSI_IMPLEMENTATION
#include "vulkan-fg/wsi_abi.h"

VkResult rsf_fixture_sdk_wsi_create(VkDevice device, VkSwapchainKHR* chain)
{
    return rsf_ffx_wsi_create(device, NULL, NULL, chain);
}

void rsf_fixture_sdk_wsi_destroy(VkDevice device, VkSwapchainKHR chain)
{
    rsf_ffx_wsi_destroy(device, chain, NULL);
}

VkResult rsf_fixture_sdk_wsi_present(VkDevice device, VkQueue queue)
{
    return rsf_ffx_wsi_present(device, queue, NULL);
}

VkResult rsf_fixture_sdk_wsi_acquire(VkDevice device, VkSwapchainKHR chain)
{
    return rsf_ffx_wsi_acquire(device, chain, UINT64_MAX, VK_NULL_HANDLE, VK_NULL_HANDLE, NULL);
}

VkResult rsf_fixture_sdk_wsi_images(VkDevice device, VkSwapchainKHR chain)
{
    return rsf_ffx_wsi_images(device, chain, NULL, NULL);
}

int main(void)
{
    return rsf_ffx_vulkan_wsi_dispatch_fixture();
}
