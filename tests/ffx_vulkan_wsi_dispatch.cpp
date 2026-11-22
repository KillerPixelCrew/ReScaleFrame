// SPDX-License-Identifier: MIT
#define RSF_FFX_WSI_IMPLEMENTATION
#include "vulkan-fg/wsi_abi.h"

extern "C" VkResult rsf_fixture_sdk_wsi_create(VkDevice device, VkSwapchainKHR* chain) {
    return rsf_ffx_wsi_create(device, nullptr, nullptr, chain);
}
extern "C" void rsf_fixture_sdk_wsi_destroy(VkDevice device, VkSwapchainKHR chain) {
    rsf_ffx_wsi_destroy(device, chain, nullptr);
}
extern "C" VkResult rsf_fixture_sdk_wsi_present(VkDevice device, VkQueue queue) {
    return rsf_ffx_wsi_present(device, queue, nullptr);
}
extern "C" VkResult rsf_fixture_sdk_wsi_acquire(VkDevice device, VkSwapchainKHR chain) {
    return rsf_ffx_wsi_acquire(device, chain, UINT64_MAX, VK_NULL_HANDLE, VK_NULL_HANDLE, nullptr);
}
extern "C" VkResult rsf_fixture_sdk_wsi_images(VkDevice device, VkSwapchainKHR chain) {
    return rsf_ffx_wsi_images(device, chain, nullptr, nullptr);
}

int main() { return rsf_ffx_vulkan_wsi_dispatch_fixture(); }
