// SPDX-License-Identifier: MIT
#define RSF_FFX_WSI_IMPLEMENTATION
#include "../runtime/vulkan/fsr3/wsi-dispatch.h"
#include "vulkan-fg/wsi_abi.h"
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
    const rsf_wsi_fixture_calls calls{
        [](uint32_t target, uint32_t abi, uint32_t mask, uint32_t short_record) -> int32_t {
            if (!target) return rsfFfxSetVulkanWsiDispatch(nullptr);
            const rsf_ffx_wsi_dispatch setup{
                uint32_t(sizeof(rsf_ffx_wsi_dispatch)) - short_record, abi, reinterpret_cast<VkDevice>(uintptr_t(target)),
                mask & 1 ? create : nullptr, mask & 2 ? destroy : nullptr, mask & 4 ? present : nullptr,
                mask & 8 ? acquire : nullptr, mask & 16 ? images : nullptr,
            };
            return rsfFfxSetVulkanWsiDispatch(&setup);
        },
        [](uint32_t target, uint64_t* output) -> int32_t {
            auto chain = reinterpret_cast<VkSwapchainKHR>(*output);
            const auto result = rsf_ffx_wsi::create(reinterpret_cast<VkDevice>(uintptr_t(target)), nullptr, nullptr, &chain);
            *output = reinterpret_cast<uint64_t>(chain);
            return result;
        },
        [](uint32_t target, uint64_t chain) {
            rsf_ffx_wsi::destroy(reinterpret_cast<VkDevice>(uintptr_t(target)), reinterpret_cast<VkSwapchainKHR>(chain), nullptr);
        },
        [](uint32_t target) -> int32_t {
            return rsf_ffx_wsi::present(reinterpret_cast<VkDevice>(uintptr_t(target)), nullptr, nullptr);
        },
        [](uint32_t target, uint64_t chain) -> int32_t {
            return rsf_ffx_wsi::acquire(reinterpret_cast<VkDevice>(uintptr_t(target)), reinterpret_cast<VkSwapchainKHR>(chain),
                UINT64_MAX, VK_NULL_HANDLE, VK_NULL_HANDLE, nullptr);
        },
        [](uint32_t target, uint64_t chain) -> int32_t {
            return rsf_ffx_wsi::images(reinterpret_cast<VkDevice>(uintptr_t(target)), reinterpret_cast<VkSwapchainKHR>(chain), nullptr, nullptr);
        },
        [](rsf_wsi_fixture_counts* output) {
            *output = {native_creates, native_destroys, host_creates, host_destroys,
                native_presents, host_presents, host_acquires, host_images};
        },
    };
    return rsf_ffx_vulkan_wsi_dispatch_fixture(&calls);
}
