// SPDX-License-Identifier: GPL-3.0-only
#define RSF_FFX_WSI_IMPLEMENTATION
#include "../runtime/vulkan/fsr3/wsi-dispatch.h"
#include <cstdio>
#include <cstdlib>
namespace {
uint32_t native_creates = 0, native_destroys = 0, host_creates = 0, host_destroys = 0;
uint32_t native_presents = 0, host_presents = 0;
uint32_t host_acquires = 0, host_images = 0;
VkDevice device = reinterpret_cast<VkDevice>(uintptr_t(1));
void require(bool value, const char* message) { if (!value) { std::fprintf(stderr, "%s\n", message); std::exit(1); } }
VkResult VKAPI_CALL create(VkDevice value, const VkSwapchainCreateInfoKHR*, const VkAllocationCallbacks*, VkSwapchainKHR* chain) {
    require(value == device, "host device identity"); ++host_creates; *chain = reinterpret_cast<VkSwapchainKHR>(uintptr_t(10)); return VK_SUCCESS;
}
void VKAPI_CALL destroy(VkDevice value, VkSwapchainKHR, const VkAllocationCallbacks*) { require(value == device, "host destroy device"); ++host_destroys; }
VkResult VKAPI_CALL present(VkQueue, const VkPresentInfoKHR*) { ++host_presents; return VK_SUCCESS; }
VkResult VKAPI_CALL acquire(VkDevice, VkSwapchainKHR, uint64_t, VkSemaphore, VkFence, uint32_t*) { ++host_acquires; return VK_SUCCESS; }
VkResult VKAPI_CALL images(VkDevice, VkSwapchainKHR, uint32_t*, VkImage*) { ++host_images; return VK_SUCCESS; }
}
extern "C" VKAPI_ATTR VkResult VKAPI_CALL vkCreateSwapchainKHR(VkDevice, const VkSwapchainCreateInfoKHR*, const VkAllocationCallbacks*, VkSwapchainKHR*) { ++native_creates; return VK_SUCCESS; }
extern "C" VKAPI_ATTR void VKAPI_CALL vkDestroySwapchainKHR(VkDevice, VkSwapchainKHR, const VkAllocationCallbacks*) { ++native_destroys; }
extern "C" VKAPI_ATTR VkResult VKAPI_CALL vkQueuePresentKHR(VkQueue, const VkPresentInfoKHR*) { ++native_presents; return VK_SUCCESS; }
extern "C" VKAPI_ATTR VkResult VKAPI_CALL vkAcquireNextImageKHR(VkDevice, VkSwapchainKHR, uint64_t, VkSemaphore, VkFence, uint32_t*) { return VK_SUCCESS; }
extern "C" VKAPI_ATTR VkResult VKAPI_CALL vkGetSwapchainImagesKHR(VkDevice, VkSwapchainKHR, uint32_t*, VkImage*) { return VK_SUCCESS; }
int main() {
    require(rsfFfxSetVulkanWsiDispatch(nullptr) == -1, "null dispatch setup");
    rsf_ffx_wsi_dispatch setup{sizeof(setup) - 1, 2, device, create, destroy, present, acquire, images};
    require(rsfFfxSetVulkanWsiDispatch(&setup) == -1, "short dispatch setup");
    setup.struct_size = sizeof(setup); setup.abi_version = 1;
    require(rsfFfxSetVulkanWsiDispatch(&setup) == -2, "dispatch ABI mismatch");
    setup.abi_version = 2; setup.destroy = nullptr;
    require(rsfFfxSetVulkanWsiDispatch(&setup) == -1, "partial owner refused");
    setup.destroy = destroy;
    require(rsfFfxSetVulkanWsiDispatch(&setup) == 0, "host dispatch registered");
    auto foreign = setup; foreign.device = reinterpret_cast<VkDevice>(uintptr_t(2));
    require(rsfFfxSetVulkanWsiDispatch(&foreign) == -3, "second owner cannot replace live dispatch");
    VkSwapchainKHR chain{};
    require(rsf_ffx_wsi::create(device, nullptr, nullptr, &chain) == VK_SUCCESS, "host create forwarded");
    rsf_ffx_wsi::destroy(device, chain, nullptr);
    rsf_ffx_wsi::present(device, nullptr, nullptr);
    rsf_ffx_wsi::acquire(device, chain, UINT64_MAX, VK_NULL_HANDLE, VK_NULL_HANDLE, nullptr);
    rsf_ffx_wsi::images(device, chain, nullptr, nullptr);
    require(host_acquires == 1 && host_images == 1, "host acquire and image query route");
    rsf_ffx_wsi::create(foreign.device, nullptr, nullptr, &chain); rsf_ffx_wsi::destroy(foreign.device, chain, nullptr);
    require(host_creates == 1 && host_destroys == 1 && native_creates == 1 && native_destroys == 1, "host device route and independent native device");
    require(host_presents == 1 && native_presents == 0, "host present synchronization callback");
    setup.create = nullptr; setup.destroy = nullptr; setup.present = nullptr; setup.acquire = nullptr; setup.images = nullptr;
    require(rsfFfxSetVulkanWsiDispatch(&setup) == 0, "drained owner unregistered");
    rsf_ffx_wsi::create(device, nullptr, nullptr, &chain); rsf_ffx_wsi::destroy(device, chain, nullptr);
    require(native_creates == 2 && native_destroys == 2 && host_creates == 1, "unregistered callbacks are never reused");
    rsf_ffx_wsi::present(device, nullptr, nullptr);
    require(native_presents == 1 && host_presents == 1, "native present after callback retirement");
    return 0;
}
