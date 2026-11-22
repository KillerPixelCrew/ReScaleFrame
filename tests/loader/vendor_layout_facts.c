/* SPDX-License-Identifier: MIT */
/* Extract SDK facts using the actual vendor declarations that compile as C. The Streamline and
   FSR 2 declarations are C++ only and are measured in vendor_layout_facts.cpp. */

#include <windows.h>

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#if defined(RSF_CENSUS_LAYOUT_FFX)
#include <dx12/ffx_api_dx12.h>
#include <dx12/ffx_api_framegeneration_dx12.h>
#include <ffx_api.h>
#include <ffx_framegeneration.h>
#include <ffx_upscale.h>
#endif
#if defined(RSF_CENSUS_LAYOUT_XESS)
#include <xess_d3d12.h>
#endif
#if defined(RSF_CENSUS_LAYOUT_VK)
#include <vulkan/vulkan.h>
#endif

typedef void (*Visit)(void*, uint32_t, const size_t*, size_t);
typedef void (*Collect)(void*, Visit);
int rsf_test_census_layouts(Collect);
#if defined(RSF_CENSUS_LAYOUT_CXX)
/* Streamline and FSR 2 facts, measured by the C++ declarations. */
void rsf_test_layout_facts_cxx(void* context, Visit visit);
#endif

#define OFFSET(Type, member) offsetof(Type, member)

static void collect(void* context, Visit visit)
{
    (void)context;
    (void)visit;
#if defined(RSF_CENSUS_LAYOUT_CXX)
    rsf_test_layout_facts_cxx(context, visit);
#endif
#if defined(RSF_CENSUS_LAYOUT_FFX)
    {
        const size_t values[] = {
            sizeof(struct FfxApiResource),
            OFFSET(struct ffxDispatchDescUpscale, commandList),
            OFFSET(struct ffxDispatchDescUpscale, color),
            OFFSET(struct ffxDispatchDescUpscale, output),
            OFFSET(struct ffxDispatchDescUpscale, jitterOffset),
            OFFSET(struct ffxDispatchDescUpscale, renderSize),
            OFFSET(struct ffxDispatchDescUpscale, upscaleSize),
            OFFSET(struct ffxDispatchDescUpscale, enableSharpening),
            OFFSET(struct ffxDispatchDescUpscale, sharpness),
            OFFSET(struct ffxDispatchDescUpscale, reset),
            OFFSET(struct ffxDispatchDescUpscale, cameraNear),
            OFFSET(struct ffxDispatchDescUpscale, viewSpaceToMetersFactor),
            OFFSET(struct ffxDispatchDescUpscale, flags),
            sizeof(struct ffxDispatchDescUpscale),
            OFFSET(struct ffxDispatchDescFrameGeneration, presentColor),
            OFFSET(struct ffxDispatchDescFrameGeneration, numGeneratedFrames),
            OFFSET(struct ffxDispatchDescFrameGeneration, reset),
            OFFSET(struct ffxDispatchDescFrameGeneration, frameID),
            sizeof(struct ffxDispatchDescFrameGeneration),
            OFFSET(struct ffxConfigureDescFrameGeneration, swapChain),
            OFFSET(struct ffxConfigureDescFrameGeneration, frameGenerationEnabled),
            OFFSET(struct ffxConfigureDescFrameGeneration, HUDLessColor),
            OFFSET(struct ffxConfigureDescFrameGeneration, flags),
            OFFSET(struct ffxConfigureDescFrameGeneration, frameID),
            OFFSET(struct ffxDispatchDescFrameGenerationPrepareV2, frameID),
            OFFSET(struct ffxDispatchDescFrameGenerationPrepareV2, commandList),
            OFFSET(struct ffxDispatchDescFrameGenerationPrepareV2, renderSize),
            OFFSET(struct ffxDispatchDescFrameGenerationPrepareV2, depth),
            OFFSET(struct ffxCreateContextDescUpscale, flags),
            OFFSET(struct ffxCreateContextDescUpscale, maxRenderSize),
            OFFSET(struct ffxCreateContextDescUpscale, maxUpscaleSize),
            OFFSET(struct ffxCreateContextDescFrameGeneration, flags),
            OFFSET(struct ffxCreateContextDescFrameGeneration, displaySize),
            OFFSET(struct ffxCreateContextDescFrameGeneration, maxRenderSize),
            OFFSET(struct ffxCreateContextDescFrameGeneration, backBufferFormat),
            OFFSET(struct ffxCreateContextDescFrameGenerationSwapChainWrapDX12, swapchain),
            OFFSET(struct ffxCreateContextDescFrameGenerationSwapChainWrapDX12, gameQueue),
            OFFSET(struct ffxCreateContextDescFrameGenerationSwapChainNewDX12, desc),
            OFFSET(struct ffxCreateContextDescFrameGenerationSwapChainNewDX12, gameQueue),
            OFFSET(struct ffxCreateContextDescFrameGenerationSwapChainForHwndDX12, hwnd),
            OFFSET(struct ffxCreateContextDescFrameGenerationSwapChainForHwndDX12, desc),
            OFFSET(struct ffxCreateContextDescFrameGenerationSwapChainForHwndDX12, gameQueue),
            OFFSET(struct ffxCreateBackendDX12Desc, device),
            FFX_API_DISPATCH_DESC_TYPE_UPSCALE,
            FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION,
            FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION,
            FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_V2,
            FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_NEW_DX12,
            FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_FOR_HWND_DX12,
            FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12,
        };
        visit(context, 1, values, sizeof(values) / sizeof(values[0]));
    }
#endif

#if defined(RSF_CENSUS_LAYOUT_XESS)
    {
        const size_t values[] = {
            OFFSET(xess_d3d12_execute_params_t, pColorTexture),
            OFFSET(xess_d3d12_execute_params_t, pOutputTexture),
            OFFSET(xess_d3d12_execute_params_t, jitterOffsetX),
            OFFSET(xess_d3d12_execute_params_t, exposureScale),
            OFFSET(xess_d3d12_execute_params_t, resetHistory),
            OFFSET(xess_d3d12_execute_params_t, inputWidth),
            OFFSET(xess_d3d12_execute_params_t, inputColorBase),
            OFFSET(xess_d3d12_execute_params_t, pDescriptorHeap),
            OFFSET(xess_d3d12_init_params_t, outputResolution),
            OFFSET(xess_d3d12_init_params_t, qualitySetting),
            OFFSET(xess_d3d12_init_params_t, initFlags),
            OFFSET(xess_d3d12_init_params_t, creationNodeMask),
            OFFSET(xess_d3d12_init_params_t, visibleNodeMask),
        };
        visit(context, 2, values, sizeof(values) / sizeof(values[0]));
    }
#endif

#if defined(RSF_CENSUS_LAYOUT_VK)
    {
        const size_t values[] = {
            OFFSET(VkInstanceCreateInfo, pApplicationInfo),
            OFFSET(VkInstanceCreateInfo, enabledLayerCount),
            OFFSET(VkInstanceCreateInfo, ppEnabledLayerNames),
            OFFSET(VkInstanceCreateInfo, enabledExtensionCount),
            OFFSET(VkInstanceCreateInfo, ppEnabledExtensionNames),
            OFFSET(VkApplicationInfo, pApplicationName),
            OFFSET(VkApplicationInfo, applicationVersion),
            OFFSET(VkApplicationInfo, pEngineName),
            OFFSET(VkApplicationInfo, engineVersion),
            OFFSET(VkApplicationInfo, apiVersion),
            OFFSET(VkDeviceCreateInfo, queueCreateInfoCount),
            OFFSET(VkDeviceCreateInfo, pQueueCreateInfos),
            OFFSET(VkDeviceCreateInfo, enabledLayerCount),
            OFFSET(VkDeviceCreateInfo, enabledExtensionCount),
            OFFSET(VkDeviceCreateInfo, ppEnabledExtensionNames),
            sizeof(VkDeviceQueueCreateInfo),
            OFFSET(VkDeviceQueueCreateInfo, queueFamilyIndex),
            OFFSET(VkDeviceQueueCreateInfo, queueCount),
            OFFSET(VkDeviceQueueInfo2, queueFamilyIndex),
            OFFSET(VkDeviceQueueInfo2, queueIndex),
            sizeof(VkSubmitInfo),
            OFFSET(VkSubmitInfo, waitSemaphoreCount),
            OFFSET(VkSubmitInfo, commandBufferCount),
            OFFSET(VkSubmitInfo, signalSemaphoreCount),
            sizeof(VkSubmitInfo2),
            OFFSET(VkSubmitInfo2, waitSemaphoreInfoCount),
            OFFSET(VkSubmitInfo2, commandBufferInfoCount),
            OFFSET(VkSubmitInfo2, signalSemaphoreInfoCount),
            OFFSET(VkPresentInfoKHR, waitSemaphoreCount),
            OFFSET(VkPresentInfoKHR, swapchainCount),
            OFFSET(VkPresentInfoKHR, pSwapchains),
            OFFSET(VkPresentInfoKHR, pImageIndices),
            OFFSET(VkSwapchainCreateInfoKHR, surface),
            OFFSET(VkSwapchainCreateInfoKHR, minImageCount),
            OFFSET(VkSwapchainCreateInfoKHR, imageFormat),
            OFFSET(VkSwapchainCreateInfoKHR, imageColorSpace),
            OFFSET(VkSwapchainCreateInfoKHR, imageExtent),
            OFFSET(VkSwapchainCreateInfoKHR, imageArrayLayers),
            OFFSET(VkSwapchainCreateInfoKHR, imageUsage),
            OFFSET(VkSwapchainCreateInfoKHR, imageSharingMode),
            OFFSET(VkSwapchainCreateInfoKHR, preTransform),
            OFFSET(VkSwapchainCreateInfoKHR, compositeAlpha),
            OFFSET(VkSwapchainCreateInfoKHR, presentMode),
            OFFSET(VkSwapchainCreateInfoKHR, clipped),
            OFFSET(VkSwapchainCreateInfoKHR, oldSwapchain),
        };
        visit(context, 3, values, sizeof(values) / sizeof(values[0]));
    }
#endif

}

int main(void) { return rsf_test_census_layouts(collect); }