/* SPDX-License-Identifier: MIT */
/* The corrected SDK's C11 WSI dispatch section: registration refusals, host routing for the owner
   device, the loader entries for a foreign device and after the owner unregisters. */
#define RSF_FFX_WSI_IMPLEMENTATION
#include "../runtime/vulkan/fsr3/wsi-dispatch.h"
#include <stdio.h>
#include <stdlib.h>

static uint32_t native_creates, native_destroys, native_presents;
static uint32_t host_creates, host_destroys, host_presents, host_acquires, host_images;
#define DEVICE ((VkDevice)(uintptr_t)1)
#define FOREIGN ((VkDevice)(uintptr_t)2)
#define QUEUE ((VkQueue)(uintptr_t)22)

static void require(int value, const char* message)
{
    if (!value) {
        fprintf(stderr, "%s\n", message);
        exit(1);
    }
}

static VkResult VKAPI_CALL host_create(VkDevice device, const VkSwapchainCreateInfoKHR* info,
    const VkAllocationCallbacks* allocator, VkSwapchainKHR* chain)
{
    (void)info, (void)allocator;
    require(device == DEVICE, "host create device identity");
    ++host_creates;
    *chain = (VkSwapchainKHR)(uintptr_t)10;
    return VK_SUCCESS;
}
static void VKAPI_CALL host_destroy(VkDevice device, VkSwapchainKHR chain, const VkAllocationCallbacks* allocator)
{
    (void)chain, (void)allocator;
    require(device == DEVICE, "host destroy device identity");
    ++host_destroys;
}
static VkResult VKAPI_CALL host_present(VkQueue queue, const VkPresentInfoKHR* info)
{
    (void)info;
    require(queue == QUEUE, "host present queue identity");
    ++host_presents;
    return VK_SUCCESS;
}
static VkResult VKAPI_CALL host_acquire(VkDevice device, VkSwapchainKHR chain, uint64_t timeout,
    VkSemaphore semaphore, VkFence fence, uint32_t* index)
{
    (void)device, (void)chain, (void)timeout, (void)semaphore, (void)fence, (void)index;
    ++host_acquires;
    return VK_SUCCESS;
}
static VkResult VKAPI_CALL host_images_query(VkDevice device, VkSwapchainKHR chain, uint32_t* count, VkImage* images)
{
    (void)device, (void)chain, (void)count, (void)images;
    ++host_images;
    return VK_SUCCESS;
}

VKAPI_ATTR VkResult VKAPI_CALL vkCreateSwapchainKHR(VkDevice device, const VkSwapchainCreateInfoKHR* info,
    const VkAllocationCallbacks* allocator, VkSwapchainKHR* chain)
{
    (void)device, (void)info, (void)allocator;
    ++native_creates;
    *chain = (VkSwapchainKHR)(uintptr_t)20;
    return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL vkDestroySwapchainKHR(VkDevice device, VkSwapchainKHR chain,
    const VkAllocationCallbacks* allocator)
{
    (void)device, (void)chain, (void)allocator;
    ++native_destroys;
}
VKAPI_ATTR VkResult VKAPI_CALL vkQueuePresentKHR(VkQueue queue, const VkPresentInfoKHR* info)
{
    (void)queue, (void)info;
    ++native_presents;
    return VK_SUBOPTIMAL_KHR;
}
VKAPI_ATTR VkResult VKAPI_CALL vkAcquireNextImageKHR(VkDevice device, VkSwapchainKHR chain, uint64_t timeout,
    VkSemaphore semaphore, VkFence fence, uint32_t* index)
{
    (void)device, (void)chain, (void)timeout, (void)semaphore, (void)fence, (void)index;
    return VK_TIMEOUT;
}
VKAPI_ATTR VkResult VKAPI_CALL vkGetSwapchainImagesKHR(VkDevice device, VkSwapchainKHR chain, uint32_t* count,
    VkImage* images)
{
    (void)device, (void)chain, (void)count, (void)images;
    return VK_INCOMPLETE;
}

static VkResult acquire(VkDevice device, VkSwapchainKHR chain)
{
    return rsf_ffx_wsi_acquire(device, chain, UINT64_MAX, VK_NULL_HANDLE, VK_NULL_HANDLE, NULL);
}

int main(void)
{
    rsf_ffx_wsi_dispatch setup = {sizeof(setup) - 1, 2, DEVICE, host_create, host_destroy, host_present,
        host_acquire, host_images_query};
    require(rsfFfxSetVulkanWsiDispatch(NULL) == -1, "null dispatch setup");
    require(rsfFfxSetVulkanWsiDispatch(&setup) == -1, "short dispatch setup");
    setup.struct_size = sizeof(setup);
    setup.abi_version = 1;
    require(rsfFfxSetVulkanWsiDispatch(&setup) == -2, "dispatch ABI mismatch");
    setup.abi_version = 2;
    setup.destroy = NULL;
    require(rsfFfxSetVulkanWsiDispatch(&setup) == -1, "partial owner refused");
    setup.destroy = host_destroy;
    require(rsfFfxSetVulkanWsiDispatch(&setup) == 0, "host dispatch registered");
    rsf_ffx_wsi_dispatch foreign = setup;
    foreign.device = FOREIGN;
    require(rsfFfxSetVulkanWsiDispatch(&foreign) == -3, "second owner cannot replace live dispatch");

    VkSwapchainKHR chain = VK_NULL_HANDLE;
    require(rsf_ffx_wsi_create(DEVICE, NULL, NULL, &chain) == VK_SUCCESS && chain == (VkSwapchainKHR)(uintptr_t)10,
        "host create returns its swapchain");
    rsf_ffx_wsi_destroy(DEVICE, chain, NULL);
    require(rsf_ffx_wsi_present(DEVICE, QUEUE, NULL) == VK_SUCCESS && acquire(DEVICE, chain) == VK_SUCCESS &&
            rsf_ffx_wsi_images(DEVICE, chain, NULL, NULL) == VK_SUCCESS,
        "present, acquire and image queries forward their results");
    require(rsf_ffx_wsi_create(FOREIGN, NULL, NULL, &chain) == VK_SUCCESS && chain == (VkSwapchainKHR)(uintptr_t)20,
        "a foreign device uses the loader entry");
    rsf_ffx_wsi_destroy(FOREIGN, chain, NULL);
    require(host_creates == 1 && host_destroys == 1 && native_creates == 1 && native_destroys == 1 &&
            host_acquires == 1 && host_images == 1,
        "host device route and independent native device");
    require(host_presents == 1 && native_presents == 0, "host present synchronization callback");

    setup.create = NULL;
    setup.destroy = NULL;
    setup.present = NULL;
    setup.acquire = NULL;
    setup.images = NULL;
    require(rsfFfxSetVulkanWsiDispatch(&setup) == 0, "drained owner unregistered");
    require(rsf_ffx_wsi_create(DEVICE, NULL, NULL, &chain) == VK_SUCCESS && chain == (VkSwapchainKHR)(uintptr_t)20,
        "native create after callback retirement");
    rsf_ffx_wsi_destroy(DEVICE, chain, NULL);
    require(rsf_ffx_wsi_present(DEVICE, QUEUE, NULL) == VK_SUBOPTIMAL_KHR && acquire(DEVICE, chain) == VK_TIMEOUT &&
            rsf_ffx_wsi_images(DEVICE, chain, NULL, NULL) == VK_INCOMPLETE,
        "native WSI results after callback retirement");
    require(native_creates == 2 && native_destroys == 2 && native_presents == 1 && host_creates == 1 &&
            host_destroys == 1 && host_presents == 1,
        "unregistered callbacks are never reused");
    return 0;
}
