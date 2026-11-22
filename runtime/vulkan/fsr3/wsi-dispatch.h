/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>
#include <vulkan/vulkan.h>

/* Private C ABI for the corrected SDK build. Callbacks borrow the host module
   until the drained owner unregisters after swapchain destruction. */
typedef struct rsf_ffx_wsi_dispatch {
    uint32_t struct_size, abi_version;
    VkDevice device;
    PFN_vkCreateSwapchainKHR create;
    PFN_vkDestroySwapchainKHR destroy;
    PFN_vkQueuePresentKHR present;
    PFN_vkAcquireNextImageKHR acquire;
    PFN_vkGetSwapchainImagesKHR images;
} rsf_ffx_wsi_dispatch;
typedef int32_t (*rsf_ffx_set_wsi_dispatch)(const rsf_ffx_wsi_dispatch*);

/* One translation unit of the corrected SDK defines this before inclusion. The
   section is C11 and also compiles inside that C++ unit. Calls for another
   device, or with no registration, use the loader's WSI entries. */
#if defined(RSF_FFX_WSI_IMPLEMENTATION)
#include <windows.h>

static SRWLOCK rsf_ffx_wsi_lock = SRWLOCK_INIT;
static rsf_ffx_wsi_dispatch rsf_ffx_wsi_state;

static inline rsf_ffx_wsi_dispatch rsf_ffx_wsi_get(VkDevice device)
{
    static const rsf_ffx_wsi_dispatch none = {0};
    rsf_ffx_wsi_dispatch entry;
    AcquireSRWLockExclusive(&rsf_ffx_wsi_lock);
    entry = rsf_ffx_wsi_state.device == device ? rsf_ffx_wsi_state : none;
    ReleaseSRWLockExclusive(&rsf_ffx_wsi_lock);
    return entry;
}

static inline VkResult rsf_ffx_wsi_create(VkDevice device, const VkSwapchainCreateInfoKHR* info,
    const VkAllocationCallbacks* allocator, VkSwapchainKHR* result)
{
    const rsf_ffx_wsi_dispatch entry = rsf_ffx_wsi_get(device);
    return entry.create ? entry.create(device, info, allocator, result)
                        : vkCreateSwapchainKHR(device, info, allocator, result);
}

static inline void rsf_ffx_wsi_destroy(VkDevice device, VkSwapchainKHR chain, const VkAllocationCallbacks* allocator)
{
    const rsf_ffx_wsi_dispatch entry = rsf_ffx_wsi_get(device);
    if (entry.destroy) entry.destroy(device, chain, allocator);
    else vkDestroySwapchainKHR(device, chain, allocator);
}

static inline VkResult rsf_ffx_wsi_present(VkDevice device, VkQueue queue, const VkPresentInfoKHR* info)
{
    const rsf_ffx_wsi_dispatch entry = rsf_ffx_wsi_get(device);
    return entry.present ? entry.present(queue, info) : vkQueuePresentKHR(queue, info);
}

static inline VkResult rsf_ffx_wsi_acquire(VkDevice device, VkSwapchainKHR chain, uint64_t timeout,
    VkSemaphore semaphore, VkFence fence, uint32_t* index)
{
    const rsf_ffx_wsi_dispatch entry = rsf_ffx_wsi_get(device);
    return entry.acquire ? entry.acquire(device, chain, timeout, semaphore, fence, index)
                         : vkAcquireNextImageKHR(device, chain, timeout, semaphore, fence, index);
}

static inline VkResult rsf_ffx_wsi_images(VkDevice device, VkSwapchainKHR chain, uint32_t* count, VkImage* images)
{
    const rsf_ffx_wsi_dispatch entry = rsf_ffx_wsi_get(device);
    return entry.images ? entry.images(device, chain, count, images)
                        : vkGetSwapchainImagesKHR(device, chain, count, images);
}

/* Registers all five entries for one device, or clears them when create is
   null. -1 rejects a malformed record, -2 another ABI and -3 a second device. */
#ifdef __cplusplus
extern "C"
#endif
__declspec(dllexport) int32_t rsfFfxSetVulkanWsiDispatch(const rsf_ffx_wsi_dispatch* setup)
{
    static const rsf_ffx_wsi_dispatch none = {0};
    int32_t result = 0;
    if (!setup || setup->struct_size < sizeof(*setup) || !setup->device) return -1;
    if (setup->abi_version != 2) return -2;
    if ((setup->create == NULL) != (setup->destroy == NULL)) return -1;
    if ((setup->create == NULL) != (setup->present == NULL)) return -1;
    if ((setup->create == NULL) != (setup->acquire == NULL) || (setup->create == NULL) != (setup->images == NULL))
        return -1;
    AcquireSRWLockExclusive(&rsf_ffx_wsi_lock);
    if (rsf_ffx_wsi_state.device && rsf_ffx_wsi_state.device != setup->device) result = -3;
    else rsf_ffx_wsi_state = setup->create ? *setup : none;
    ReleaseSRWLockExclusive(&rsf_ffx_wsi_lock);
    return result;
}
#endif
