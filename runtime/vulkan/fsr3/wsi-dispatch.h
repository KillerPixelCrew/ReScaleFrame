// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <stdint.h>
#include <vulkan/vulkan.h>

// Private C ABI for the corrected SDK build. Callbacks borrow the host module
// until the drained owner unregisters after swapchain destruction.
struct rsf_ffx_wsi_dispatch {
    uint32_t struct_size, abi_version;
    VkDevice device;
    PFN_vkCreateSwapchainKHR create;
    PFN_vkDestroySwapchainKHR destroy;
    PFN_vkQueuePresentKHR present;
    PFN_vkAcquireNextImageKHR acquire;
    PFN_vkGetSwapchainImagesKHR images;
};
using rsf_ffx_set_wsi_dispatch = int32_t (*)(const rsf_ffx_wsi_dispatch*);

#if defined(RSF_FFX_WSI_IMPLEMENTATION)
#include <mutex>
namespace rsf_ffx_wsi {
std::mutex mutex;
rsf_ffx_wsi_dispatch dispatch{};
rsf_ffx_wsi_dispatch get(VkDevice device) {
    std::lock_guard<std::mutex> lock(mutex);
    return dispatch.device == device ? dispatch : rsf_ffx_wsi_dispatch{};
}
VkResult create(VkDevice device, const VkSwapchainCreateInfoKHR* info, const VkAllocationCallbacks* allocator, VkSwapchainKHR* result) {
    auto entry = get(device);
    return entry.create ? entry.create(device, info, allocator, result) : vkCreateSwapchainKHR(device, info, allocator, result);
}
void destroy(VkDevice device, VkSwapchainKHR chain, const VkAllocationCallbacks* allocator) {
    auto entry = get(device);
    if (entry.destroy) entry.destroy(device, chain, allocator);
    else vkDestroySwapchainKHR(device, chain, allocator);
}
VkResult present(VkDevice device, VkQueue queue, const VkPresentInfoKHR* info) {
    auto entry = get(device);
    return entry.present ? entry.present(queue, info) : vkQueuePresentKHR(queue, info);
}
VkResult acquire(VkDevice device, VkSwapchainKHR chain, uint64_t timeout, VkSemaphore semaphore, VkFence fence, uint32_t* index) {
    auto entry = get(device);
    return entry.acquire ? entry.acquire(device, chain, timeout, semaphore, fence, index) : vkAcquireNextImageKHR(device, chain, timeout, semaphore, fence, index);
}
VkResult images(VkDevice device, VkSwapchainKHR chain, uint32_t* count, VkImage* images) {
    auto entry = get(device);
    return entry.images ? entry.images(device, chain, count, images) : vkGetSwapchainImagesKHR(device, chain, count, images);
}
}
extern "C" __declspec(dllexport) int32_t rsfFfxSetVulkanWsiDispatch(const rsf_ffx_wsi_dispatch* setup) {
    if (!setup || setup->struct_size < sizeof(*setup) || !setup->device) return -1;
    if (setup->abi_version != 2) return -2;
    if ((setup->create == nullptr) != (setup->destroy == nullptr)) return -1;
    if ((setup->create == nullptr) != (setup->present == nullptr)) return -1;
    if ((setup->create == nullptr) != (setup->acquire == nullptr) || (setup->create == nullptr) != (setup->images == nullptr)) return -1;
    std::lock_guard<std::mutex> lock(rsf_ffx_wsi::mutex);
    if (rsf_ffx_wsi::dispatch.device && rsf_ffx_wsi::dispatch.device != setup->device) return -3;
    rsf_ffx_wsi::dispatch = setup->create ? *setup : rsf_ffx_wsi_dispatch{};
    return 0;
}
#endif
