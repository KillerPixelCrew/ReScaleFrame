// SPDX-License-Identifier: MIT
#pragma once
#include <vulkan/vulkan.h>

#ifdef __cplusplus
extern "C" {
#endif

struct VkWin32SurfaceCreateInfoKHR;
typedef int32_t (*rsf_slvk_legacy_wsi_callback)(void*);

/* Vulkan dispatch from the Rust interposer owner. The host retires all
   dispatch uses before unloading. Vulkan handles and structures are borrowed. */
PFN_vkVoidFunction rsf_vk_sl_get_instance_proc_addr(VkInstance, const char*);
PFN_vkVoidFunction rsf_vk_sl_get_device_proc_addr(VkDevice, const char*);
VkResult rsf_vk_sl_create_instance(const VkInstanceCreateInfo*, VkInstance*);
VkResult rsf_vk_sl_enumerate_physical_devices(VkInstance, uint32_t*, VkPhysicalDevice*);
VkResult rsf_vk_sl_create_device(VkInstance, VkPhysicalDevice, const VkDeviceCreateInfo*, VkDevice*);
VkResult rsf_vk_sl_create_win32_surface(VkInstance, const struct VkWin32SurfaceCreateInfoKHR*, VkSurfaceKHR*);
void rsf_vk_sl_destroy_surface(VkInstance, VkSurfaceKHR);
VkResult rsf_vk_sl_create_swapchain(VkDevice, const VkSwapchainCreateInfoKHR*, VkSwapchainKHR*);
void rsf_vk_sl_destroy_swapchain(VkDevice, VkSwapchainKHR);
VkResult rsf_vk_sl_get_swapchain_images(VkDevice, VkSwapchainKHR, uint32_t*, VkImage*);
VkResult rsf_vk_sl_acquire(VkDevice, VkSwapchainKHR, uint64_t, VkSemaphore, VkFence, uint32_t*);
VkResult rsf_vk_sl_present(VkDevice, VkQueue, const VkPresentInfoKHR*);
VkResult rsf_vk_sl_device_wait_idle(VkDevice);
int32_t rsf_vk_sl_queue_present_synchronized(uintptr_t, void*, const void*);
int32_t rsf_vk_sl_vulkan_wsi_call(rsf_slvk_legacy_wsi_callback, void*);
int32_t rsf_vk_sl_without_dlss_g(rsf_slvk_legacy_wsi_callback, void*);

#ifdef __cplusplus
}
#endif
