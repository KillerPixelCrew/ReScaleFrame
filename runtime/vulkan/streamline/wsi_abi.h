// SPDX-License-Identifier: MIT
#pragma once
#include <vulkan/vulkan.h>

#ifdef __cplusplus
extern "C" {
#endif

struct VkWin32SurfaceCreateInfoKHR;
typedef int32_t (*rsf_slvk_legacy_wsi_callback)(void*);

/* Compatibility exports from the Rust interposer owner. The host retires all
   dispatch uses before unloading. Vulkan handles and structures are borrowed. */
PFN_vkVoidFunction VulkanStorySlGetInstanceProcAddr(VkInstance, const char*);
PFN_vkVoidFunction VulkanStorySlGetDeviceProcAddr(VkDevice, const char*);
VkResult VulkanStorySlCreateInstance(const VkInstanceCreateInfo*, VkInstance*);
VkResult VulkanStorySlEnumeratePhysicalDevices(VkInstance, uint32_t*, VkPhysicalDevice*);
VkResult VulkanStorySlCreateDevice(VkInstance, VkPhysicalDevice, const VkDeviceCreateInfo*, VkDevice*);
VkResult VulkanStorySlCreateWin32Surface(VkInstance, const struct VkWin32SurfaceCreateInfoKHR*, VkSurfaceKHR*);
void VulkanStorySlDestroySurface(VkInstance, VkSurfaceKHR);
VkResult VulkanStorySlCreateSwapchain(VkDevice, const VkSwapchainCreateInfoKHR*, VkSwapchainKHR*);
void VulkanStorySlDestroySwapchain(VkDevice, VkSwapchainKHR);
VkResult VulkanStorySlGetSwapchainImages(VkDevice, VkSwapchainKHR, uint32_t*, VkImage*);
VkResult VulkanStorySlAcquire(VkDevice, VkSwapchainKHR, uint64_t, VkSemaphore, VkFence, uint32_t*);
VkResult VulkanStorySlPresent(VkDevice, VkQueue, const VkPresentInfoKHR*);
VkResult VulkanStorySlDeviceWaitIdle(VkDevice);
int32_t VulkanStorySlQueuePresentSynchronized(uintptr_t, void*, const void*);
int32_t VulkanStorySlVulkanWsiCall(rsf_slvk_legacy_wsi_callback, void*);
int32_t VulkanStorySlWithoutDlssG(rsf_slvk_legacy_wsi_callback, void*);

#ifdef __cplusplus
}
#endif
