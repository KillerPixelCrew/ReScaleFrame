// SPDX-License-Identifier: MIT
#define RSF_FFX_WSI_IMPLEMENTATION
#include "../runtime/vulkan/fsr3/wsi-dispatch.h"
#include "vulkan-fg/wsi_abi.h"
extern "C" {
VkResult VKAPI_CALL rsf_fixture_host_create(VkDevice, const VkSwapchainCreateInfoKHR*, const VkAllocationCallbacks*, VkSwapchainKHR*);
void VKAPI_CALL rsf_fixture_host_destroy(VkDevice, VkSwapchainKHR, const VkAllocationCallbacks*);
VkResult VKAPI_CALL rsf_fixture_host_present(VkQueue, const VkPresentInfoKHR*);
VkResult VKAPI_CALL rsf_fixture_host_acquire(VkDevice, VkSwapchainKHR, uint64_t, VkSemaphore, VkFence, uint32_t*);
VkResult VKAPI_CALL rsf_fixture_host_images(VkDevice, VkSwapchainKHR, uint32_t*, VkImage*);
void rsf_fixture_wsi_counts(rsf_wsi_fixture_counts*);
}
int main() {
    const rsf_wsi_fixture_calls calls{
        [](uint32_t target, uint32_t abi, uint32_t mask, uint32_t short_record) -> int32_t {
            if (!target) return rsfFfxSetVulkanWsiDispatch(nullptr);
            const rsf_ffx_wsi_dispatch setup{
                uint32_t(sizeof(rsf_ffx_wsi_dispatch)) - short_record, abi, reinterpret_cast<VkDevice>(uintptr_t(target)),
                mask & 1 ? rsf_fixture_host_create : nullptr, mask & 2 ? rsf_fixture_host_destroy : nullptr, mask & 4 ? rsf_fixture_host_present : nullptr,
                mask & 8 ? rsf_fixture_host_acquire : nullptr, mask & 16 ? rsf_fixture_host_images : nullptr,
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
        rsf_fixture_wsi_counts,
    };
    return rsf_ffx_vulkan_wsi_dispatch_fixture(&calls);
}
