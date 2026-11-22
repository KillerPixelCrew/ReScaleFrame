// SPDX-License-Identifier: MIT
// Extract SDK facts using the actual vendor declarations.

#include <windows.h>

#include <cstddef>
#include <cstdint>

#if defined(RSF_CENSUS_LAYOUT_SL)
#include <sl.h>
#include <sl_consts.h>
#include <sl_dlss.h>
#include <sl_dlss_g.h>
#include <sl_pcl.h>
#include <sl_reflex.h>
#endif
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
#if defined(RSF_CENSUS_LAYOUT_FSR2)
#include <ffx_fsr2.h>
#endif

using Visit = void (*)(void*, uint32_t, const size_t*, size_t);
using Collect = void (*)(void*, Visit);
extern "C" int rsf_test_census_layouts(Collect);

namespace {

// Offsets of members of types with constructors or virtual functions: measure on real storage.
template <class T, class M>
size_t member_offset(M T::*member) {
    alignas(64) static unsigned char storage[8192];
    T* object = reinterpret_cast<T*>(storage);
    return static_cast<size_t>(reinterpret_cast<unsigned char*>(&(object->*member)) - storage);
}

#define OFFSET(Type, member) member_offset(&Type::member)

void collect(void* context, Visit visit) {
    (void)context;
    (void)visit;
#if defined(RSF_CENSUS_LAYOUT_SL)
    {
        const size_t values[] = {
            sizeof(sl::BaseStructure),
            OFFSET(sl::Preferences, showConsole),
            OFFSET(sl::Preferences, logLevel),
            OFFSET(sl::Preferences, numPathsToPlugins),
            OFFSET(sl::Preferences, pathToLogsAndData),
            OFFSET(sl::Preferences, flags),
            OFFSET(sl::Preferences, featuresToLoad),
            OFFSET(sl::Preferences, numFeaturesToLoad),
            OFFSET(sl::Preferences, applicationId),
            OFFSET(sl::Preferences, engine),
            OFFSET(sl::Preferences, engineVersion),
            OFFSET(sl::Preferences, projectId),
            OFFSET(sl::Preferences, renderAPI),
            OFFSET(sl::ResourceTag, resource),
            OFFSET(sl::ResourceTag, type),
            OFFSET(sl::ResourceTag, lifecycle),
            OFFSET(sl::ResourceTag, extent),
            sizeof(sl::ResourceTag),
            OFFSET(sl::Resource, type),
            OFFSET(sl::Resource, native),
            OFFSET(sl::Resource, state),
            OFFSET(sl::Resource, width),
            OFFSET(sl::Resource, height),
            OFFSET(sl::Resource, nativeFormat),
            OFFSET(sl::Constants, jitterOffset),
            OFFSET(sl::Constants, cameraViewToClip),
            OFFSET(sl::Constants, clipToCameraView),
            OFFSET(sl::Constants, clipToLensClip),
            OFFSET(sl::Constants, clipToPrevClip),
            OFFSET(sl::Constants, prevClipToClip),
            OFFSET(sl::Constants, cameraPos),
            OFFSET(sl::Constants, cameraUp),
            OFFSET(sl::Constants, cameraRight),
            OFFSET(sl::Constants, cameraFwd),
            OFFSET(sl::Constants, mvecScale),
            OFFSET(sl::Constants, cameraNear),
            OFFSET(sl::Constants, cameraFar),
            OFFSET(sl::Constants, cameraFOV),
            OFFSET(sl::Constants, cameraAspectRatio),
            OFFSET(sl::Constants, depthInverted),
            OFFSET(sl::Constants, cameraMotionIncluded),
            OFFSET(sl::Constants, motionVectors3D),
            OFFSET(sl::Constants, reset),
            OFFSET(sl::Constants, orthographicProjection),
            OFFSET(sl::Constants, motionVectorsDilated),
            OFFSET(sl::Constants, motionVectorsJittered),
            OFFSET(sl::DLSSGOptions, mode),
            OFFSET(sl::DLSSGOptions, numFramesToGenerate),
            OFFSET(sl::DLSSGOptions, flags),
            OFFSET(sl::DLSSGOptions, dynamicResWidth),
            OFFSET(sl::DLSSGOptions, numBackBuffers),
            OFFSET(sl::DLSSGOptions, mvecDepthWidth),
            OFFSET(sl::DLSSGOptions, colorWidth),
            OFFSET(sl::DLSSGOptions, colorBufferFormat),
            OFFSET(sl::DLSSGOptions, uiBufferFormat),
            OFFSET(sl::DLSSOptions, mode),
            OFFSET(sl::DLSSOptions, outputWidth),
            OFFSET(sl::DLSSOptions, outputHeight),
            OFFSET(sl::DLSSOptions, sharpness),
            OFFSET(sl::DLSSOptions, preExposure),
            OFFSET(sl::DLSSOptions, exposureScale),
            OFFSET(sl::ReflexOptions, mode),
            OFFSET(sl::ReflexOptions, frameLimitUs),
            OFFSET(sl::ReflexOptions, useMarkersToOptimize),
            OFFSET(sl::ReflexOptions, virtualKey),
            OFFSET(sl::ReflexOptions, idThread),
            sizeof(sl::ViewportHandle),
        };
        visit(context, 0, values, sizeof(values) / sizeof(values[0]));
    }
#endif
#if defined(RSF_CENSUS_LAYOUT_FFX)
    {
        const size_t values[] = {
            sizeof(FfxApiResource),
            OFFSET(ffxDispatchDescUpscale, commandList),
            OFFSET(ffxDispatchDescUpscale, color),
            OFFSET(ffxDispatchDescUpscale, output),
            OFFSET(ffxDispatchDescUpscale, jitterOffset),
            OFFSET(ffxDispatchDescUpscale, renderSize),
            OFFSET(ffxDispatchDescUpscale, upscaleSize),
            OFFSET(ffxDispatchDescUpscale, enableSharpening),
            OFFSET(ffxDispatchDescUpscale, sharpness),
            OFFSET(ffxDispatchDescUpscale, reset),
            OFFSET(ffxDispatchDescUpscale, cameraNear),
            OFFSET(ffxDispatchDescUpscale, viewSpaceToMetersFactor),
            OFFSET(ffxDispatchDescUpscale, flags),
            sizeof(ffxDispatchDescUpscale),
            OFFSET(ffxDispatchDescFrameGeneration, presentColor),
            OFFSET(ffxDispatchDescFrameGeneration, numGeneratedFrames),
            OFFSET(ffxDispatchDescFrameGeneration, reset),
            OFFSET(ffxDispatchDescFrameGeneration, frameID),
            sizeof(ffxDispatchDescFrameGeneration),
            OFFSET(ffxConfigureDescFrameGeneration, swapChain),
            OFFSET(ffxConfigureDescFrameGeneration, frameGenerationEnabled),
            OFFSET(ffxConfigureDescFrameGeneration, HUDLessColor),
            OFFSET(ffxConfigureDescFrameGeneration, flags),
            OFFSET(ffxConfigureDescFrameGeneration, frameID),
            OFFSET(ffxDispatchDescFrameGenerationPrepareV2, frameID),
            OFFSET(ffxDispatchDescFrameGenerationPrepareV2, commandList),
            OFFSET(ffxDispatchDescFrameGenerationPrepareV2, renderSize),
            OFFSET(ffxDispatchDescFrameGenerationPrepareV2, depth),
            OFFSET(ffxCreateContextDescUpscale, flags),
            OFFSET(ffxCreateContextDescUpscale, maxRenderSize),
            OFFSET(ffxCreateContextDescUpscale, maxUpscaleSize),
            OFFSET(ffxCreateContextDescFrameGeneration, flags),
            OFFSET(ffxCreateContextDescFrameGeneration, displaySize),
            OFFSET(ffxCreateContextDescFrameGeneration, maxRenderSize),
            OFFSET(ffxCreateContextDescFrameGeneration, backBufferFormat),
            OFFSET(ffxCreateContextDescFrameGenerationSwapChainWrapDX12, swapchain),
            OFFSET(ffxCreateContextDescFrameGenerationSwapChainWrapDX12, gameQueue),
            OFFSET(ffxCreateContextDescFrameGenerationSwapChainNewDX12, desc),
            OFFSET(ffxCreateContextDescFrameGenerationSwapChainNewDX12, gameQueue),
            OFFSET(ffxCreateContextDescFrameGenerationSwapChainForHwndDX12, hwnd),
            OFFSET(ffxCreateContextDescFrameGenerationSwapChainForHwndDX12, desc),
            OFFSET(ffxCreateContextDescFrameGenerationSwapChainForHwndDX12, gameQueue),
            OFFSET(ffxCreateBackendDX12Desc, device),
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
#if defined(RSF_CENSUS_LAYOUT_FSR2)
    {
        const size_t values[] = {
            sizeof(Fsr212::FfxResource),
            OFFSET(Fsr212::FfxFsr2DispatchDescription, color),
            OFFSET(Fsr212::FfxFsr2DispatchDescription, output),
            OFFSET(Fsr212::FfxFsr2DispatchDescription, jitterOffset),
            OFFSET(Fsr212::FfxFsr2DispatchDescription, renderSize),
            OFFSET(Fsr212::FfxFsr2DispatchDescription, enableSharpening),
            OFFSET(Fsr212::FfxFsr2DispatchDescription, reset),
            OFFSET(Fsr212::FfxFsr2DispatchDescription, cameraFovAngleVertical),
            sizeof(Fsr212::FfxFsr2DispatchDescription),
            OFFSET(Fsr212::FfxResource, name),
            OFFSET(Fsr212::FfxResource, description),
            OFFSET(Fsr212::FfxResource, state),
            OFFSET(Fsr212::FfxResource, isDepth),
        };
        visit(context, 4, values, sizeof(values) / sizeof(values[0]));
    }
#endif
}

} // namespace

int main() { return rsf_test_census_layouts(collect); }
