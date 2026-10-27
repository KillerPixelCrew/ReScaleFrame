// SPDX-License-Identifier: GPL-3.0-only
// Checks the structure offsets the census decoders use against the vendor headers in the tree
// (untracked, under vendor/ and references/). The census itself replicates the layouts so that it
// builds without those headers; this test is what keeps the copies honest. It is built only when
// the headers exist, and RSF_CENSUS_LAYOUT_* tells which sets were available.
#include <windows.h>

#include <cstddef>
#include <cstdio>
#include <cstring>

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

namespace {

int g_failures = 0;

void expect(const char* what, size_t actual, size_t expected) {
    if (actual != expected) {
        std::printf("FAIL %s: header says %zu, census uses %zu\n", what, actual, expected);
        ++g_failures;
    }
}

// Offsets of members of types with constructors or virtual functions: measure on real storage.
template <class T, class M>
size_t member_offset(M T::*member) {
    alignas(64) static unsigned char storage[8192];
    T* object = reinterpret_cast<T*>(storage);
    return static_cast<size_t>(reinterpret_cast<unsigned char*>(&(object->*member)) - storage);
}

#define OFFSET(Type, member, expected) expect(#Type "::" #member, member_offset(&Type::member), expected)

} // namespace

int main() {
    int sets = 0;
#if defined(RSF_CENSUS_LAYOUT_SL)
    ++sets;
    // wrap_misc.cpp: Streamline structures after the 32 byte sl::BaseStructure header.
    expect("sl::BaseStructure size", sizeof(sl::BaseStructure), 32);
    OFFSET(sl::Preferences, showConsole, 32);
    OFFSET(sl::Preferences, logLevel, 36);
    OFFSET(sl::Preferences, numPathsToPlugins, 48);
    OFFSET(sl::Preferences, pathToLogsAndData, 56);
    OFFSET(sl::Preferences, flags, 88);
    OFFSET(sl::Preferences, featuresToLoad, 96);
    OFFSET(sl::Preferences, numFeaturesToLoad, 104);
    OFFSET(sl::Preferences, applicationId, 108);
    OFFSET(sl::Preferences, engine, 112);
    OFFSET(sl::Preferences, engineVersion, 120);
    OFFSET(sl::Preferences, projectId, 128);
    OFFSET(sl::Preferences, renderAPI, 136);
    OFFSET(sl::ResourceTag, resource, 32);
    OFFSET(sl::ResourceTag, type, 40);
    OFFSET(sl::ResourceTag, lifecycle, 44);
    OFFSET(sl::ResourceTag, extent, 48);
    expect("sl::ResourceTag size", sizeof(sl::ResourceTag), 64);
    OFFSET(sl::Resource, type, 32);
    OFFSET(sl::Resource, native, 40);
    OFFSET(sl::Resource, state, 64);
    OFFSET(sl::Resource, width, 68);
    OFFSET(sl::Resource, height, 72);
    OFFSET(sl::Resource, nativeFormat, 76);
    OFFSET(sl::Constants, jitterOffset, 352);
    OFFSET(sl::Constants, mvecScale, 360);
    OFFSET(sl::Constants, cameraNear, 424);
    OFFSET(sl::Constants, cameraFar, 428);
    OFFSET(sl::Constants, cameraFOV, 432);
    OFFSET(sl::Constants, cameraAspectRatio, 436);
    OFFSET(sl::Constants, depthInverted, 444);
    OFFSET(sl::Constants, cameraMotionIncluded, 445);
    OFFSET(sl::Constants, motionVectors3D, 446);
    OFFSET(sl::Constants, reset, 447);
    OFFSET(sl::Constants, orthographicProjection, 448);
    OFFSET(sl::Constants, motionVectorsDilated, 449);
    OFFSET(sl::Constants, motionVectorsJittered, 450);
    OFFSET(sl::DLSSGOptions, mode, 32);
    OFFSET(sl::DLSSGOptions, numFramesToGenerate, 36);
    OFFSET(sl::DLSSGOptions, flags, 40);
    OFFSET(sl::DLSSGOptions, dynamicResWidth, 44);
    OFFSET(sl::DLSSGOptions, numBackBuffers, 52);
    OFFSET(sl::DLSSGOptions, mvecDepthWidth, 56);
    OFFSET(sl::DLSSGOptions, colorWidth, 64);
    OFFSET(sl::DLSSGOptions, colorBufferFormat, 72);
    OFFSET(sl::DLSSGOptions, uiBufferFormat, 88);
    OFFSET(sl::DLSSOptions, mode, 32);
    OFFSET(sl::DLSSOptions, outputWidth, 36);
    OFFSET(sl::DLSSOptions, outputHeight, 40);
    OFFSET(sl::DLSSOptions, sharpness, 44);
    OFFSET(sl::DLSSOptions, preExposure, 48);
    OFFSET(sl::DLSSOptions, exposureScale, 52);
    OFFSET(sl::ReflexOptions, mode, 32);
    OFFSET(sl::ReflexOptions, frameLimitUs, 36);
    OFFSET(sl::ReflexOptions, useMarkersToOptimize, 40);
    OFFSET(sl::ReflexOptions, virtualKey, 42);
    OFFSET(sl::ReflexOptions, idThread, 44);
    // The value is private; it follows the header, so the size shows where it sits.
    expect("sl::ViewportHandle size", sizeof(sl::ViewportHandle), 40);
#endif
#if defined(RSF_CENSUS_LAYOUT_FFX)
    ++sets;
    // wrap_fsr.cpp: FidelityFX API descriptors.
    expect("FfxApiResource size", sizeof(FfxApiResource), 48);
    OFFSET(ffxDispatchDescUpscale, commandList, 16);
    OFFSET(ffxDispatchDescUpscale, color, 24);
    OFFSET(ffxDispatchDescUpscale, output, 312);
    OFFSET(ffxDispatchDescUpscale, jitterOffset, 360);
    OFFSET(ffxDispatchDescUpscale, renderSize, 376);
    OFFSET(ffxDispatchDescUpscale, upscaleSize, 384);
    OFFSET(ffxDispatchDescUpscale, enableSharpening, 392);
    OFFSET(ffxDispatchDescUpscale, sharpness, 396);
    OFFSET(ffxDispatchDescUpscale, reset, 408);
    OFFSET(ffxDispatchDescUpscale, cameraNear, 412);
    OFFSET(ffxDispatchDescUpscale, viewSpaceToMetersFactor, 424);
    OFFSET(ffxDispatchDescUpscale, flags, 428);
    expect("ffxDispatchDescUpscale size", sizeof(ffxDispatchDescUpscale), 432);
    OFFSET(ffxDispatchDescFrameGeneration, presentColor, 24);
    OFFSET(ffxDispatchDescFrameGeneration, numGeneratedFrames, 264);
    OFFSET(ffxDispatchDescFrameGeneration, reset, 268);
    OFFSET(ffxDispatchDescFrameGeneration, frameID, 304);
    expect("ffxDispatchDescFrameGeneration size", sizeof(ffxDispatchDescFrameGeneration), 312);
    OFFSET(ffxConfigureDescFrameGeneration, swapChain, 16);
    OFFSET(ffxConfigureDescFrameGeneration, frameGenerationEnabled, 56);
    OFFSET(ffxConfigureDescFrameGeneration, HUDLessColor, 64);
    OFFSET(ffxConfigureDescFrameGeneration, flags, 112);
    OFFSET(ffxConfigureDescFrameGeneration, frameID, 136);
    OFFSET(ffxDispatchDescFrameGenerationPrepareV2, frameID, 16);
    OFFSET(ffxDispatchDescFrameGenerationPrepareV2, commandList, 32);
    OFFSET(ffxDispatchDescFrameGenerationPrepareV2, renderSize, 40);
    OFFSET(ffxDispatchDescFrameGenerationPrepareV2, depth, 88);
    OFFSET(ffxCreateContextDescUpscale, flags, 16);
    OFFSET(ffxCreateContextDescUpscale, maxRenderSize, 20);
    OFFSET(ffxCreateContextDescUpscale, maxUpscaleSize, 28);
    OFFSET(ffxCreateContextDescFrameGeneration, flags, 16);
    OFFSET(ffxCreateContextDescFrameGeneration, displaySize, 20);
    OFFSET(ffxCreateContextDescFrameGeneration, maxRenderSize, 28);
    OFFSET(ffxCreateContextDescFrameGeneration, backBufferFormat, 36);
    OFFSET(ffxCreateContextDescFrameGenerationSwapChainWrapDX12, swapchain, 16);
    OFFSET(ffxCreateContextDescFrameGenerationSwapChainWrapDX12, gameQueue, 24);
    OFFSET(ffxCreateContextDescFrameGenerationSwapChainNewDX12, desc, 24);
    OFFSET(ffxCreateContextDescFrameGenerationSwapChainNewDX12, gameQueue, 40);
    OFFSET(ffxCreateContextDescFrameGenerationSwapChainForHwndDX12, hwnd, 24);
    OFFSET(ffxCreateContextDescFrameGenerationSwapChainForHwndDX12, desc, 32);
    OFFSET(ffxCreateContextDescFrameGenerationSwapChainForHwndDX12, gameQueue, 56);
    OFFSET(ffxCreateBackendDX12Desc, device, 16);
    expect("FFX_API_DISPATCH_DESC_TYPE_UPSCALE", FFX_API_DISPATCH_DESC_TYPE_UPSCALE, 0x00010001);
    expect("FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION", FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION, 0x00020003);
    expect("FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION", FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION, 0x00020002);
    expect("FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_V2", FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_V2,
           0x0002000c);
    expect("FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_NEW_DX12",
           FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_NEW_DX12, 0x00030005);
    expect("FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_FOR_HWND_DX12",
           FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_FOR_HWND_DX12, 0x00030006);
    expect("FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12", FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12, 0x00000002);
#endif
#if defined(RSF_CENSUS_LAYOUT_XESS)
    ++sets;
    // wrap_fsr.cpp: xess_d3d12_execute_params_t and the head of xess_d3d12_init_params_t.
    OFFSET(xess_d3d12_execute_params_t, pColorTexture, 0);
    OFFSET(xess_d3d12_execute_params_t, pOutputTexture, 40);
    OFFSET(xess_d3d12_execute_params_t, jitterOffsetX, 48);
    OFFSET(xess_d3d12_execute_params_t, exposureScale, 56);
    OFFSET(xess_d3d12_execute_params_t, resetHistory, 60);
    OFFSET(xess_d3d12_execute_params_t, inputWidth, 64);
    OFFSET(xess_d3d12_execute_params_t, inputColorBase, 72);
    OFFSET(xess_d3d12_execute_params_t, pDescriptorHeap, 120);
    OFFSET(xess_d3d12_init_params_t, outputResolution, 0);
    OFFSET(xess_d3d12_init_params_t, qualitySetting, 8);
    OFFSET(xess_d3d12_init_params_t, initFlags, 12);
    OFFSET(xess_d3d12_init_params_t, creationNodeMask, 16);
    OFFSET(xess_d3d12_init_params_t, visibleNodeMask, 20);
#endif
#if defined(RSF_CENSUS_LAYOUT_VK)
    ++sets;
    // wrap_vk.cpp and the Vulkan fixture in vendor_census.cpp: Vulkan structure offsets (LP64).
    OFFSET(VkInstanceCreateInfo, pApplicationInfo, 24);
    OFFSET(VkInstanceCreateInfo, enabledLayerCount, 32);
    OFFSET(VkInstanceCreateInfo, ppEnabledLayerNames, 40);
    OFFSET(VkInstanceCreateInfo, enabledExtensionCount, 48);
    OFFSET(VkInstanceCreateInfo, ppEnabledExtensionNames, 56);
    OFFSET(VkApplicationInfo, pApplicationName, 16);
    OFFSET(VkApplicationInfo, applicationVersion, 24);
    OFFSET(VkApplicationInfo, pEngineName, 32);
    OFFSET(VkApplicationInfo, engineVersion, 40);
    OFFSET(VkApplicationInfo, apiVersion, 44);
    OFFSET(VkDeviceCreateInfo, queueCreateInfoCount, 20);
    OFFSET(VkDeviceCreateInfo, pQueueCreateInfos, 24);
    OFFSET(VkDeviceCreateInfo, enabledLayerCount, 32);
    OFFSET(VkDeviceCreateInfo, enabledExtensionCount, 48);
    OFFSET(VkDeviceCreateInfo, ppEnabledExtensionNames, 56);
    expect("VkDeviceQueueCreateInfo size", sizeof(VkDeviceQueueCreateInfo), 40);
    OFFSET(VkDeviceQueueCreateInfo, queueFamilyIndex, 20);
    OFFSET(VkDeviceQueueCreateInfo, queueCount, 24);
    OFFSET(VkDeviceQueueInfo2, queueFamilyIndex, 20);
    OFFSET(VkDeviceQueueInfo2, queueIndex, 24);
    expect("VkSubmitInfo size", sizeof(VkSubmitInfo), 72);
    OFFSET(VkSubmitInfo, waitSemaphoreCount, 16);
    OFFSET(VkSubmitInfo, commandBufferCount, 40);
    OFFSET(VkSubmitInfo, signalSemaphoreCount, 56);
    expect("VkSubmitInfo2 size", sizeof(VkSubmitInfo2), 64);
    OFFSET(VkSubmitInfo2, waitSemaphoreInfoCount, 20);
    OFFSET(VkSubmitInfo2, commandBufferInfoCount, 32);
    OFFSET(VkSubmitInfo2, signalSemaphoreInfoCount, 48);
    OFFSET(VkPresentInfoKHR, waitSemaphoreCount, 16);
    OFFSET(VkPresentInfoKHR, swapchainCount, 32);
    OFFSET(VkPresentInfoKHR, pSwapchains, 40);
    OFFSET(VkPresentInfoKHR, pImageIndices, 48);
    OFFSET(VkSwapchainCreateInfoKHR, surface, 24);
    OFFSET(VkSwapchainCreateInfoKHR, minImageCount, 32);
    OFFSET(VkSwapchainCreateInfoKHR, imageFormat, 36);
    OFFSET(VkSwapchainCreateInfoKHR, imageColorSpace, 40);
    OFFSET(VkSwapchainCreateInfoKHR, imageExtent, 44);
    OFFSET(VkSwapchainCreateInfoKHR, imageArrayLayers, 52);
    OFFSET(VkSwapchainCreateInfoKHR, imageUsage, 56);
    OFFSET(VkSwapchainCreateInfoKHR, imageSharingMode, 60);
    OFFSET(VkSwapchainCreateInfoKHR, preTransform, 80);
    OFFSET(VkSwapchainCreateInfoKHR, compositeAlpha, 84);
    OFFSET(VkSwapchainCreateInfoKHR, presentMode, 88);
    OFFSET(VkSwapchainCreateInfoKHR, clipped, 92);
    OFFSET(VkSwapchainCreateInfoKHR, oldSwapchain, 96);
#endif
#if defined(RSF_CENSUS_LAYOUT_FSR2)
    ++sets;
    // wrap_fsr.cpp: FSR 2.1.2 dispatch description.
    expect("FfxResource size", sizeof(Fsr212::FfxResource), 184);
    OFFSET(Fsr212::FfxFsr2DispatchDescription, color, 8);
    OFFSET(Fsr212::FfxFsr2DispatchDescription, output, 1112);
    OFFSET(Fsr212::FfxFsr2DispatchDescription, jitterOffset, 1296);
    OFFSET(Fsr212::FfxFsr2DispatchDescription, renderSize, 1312);
    OFFSET(Fsr212::FfxFsr2DispatchDescription, enableSharpening, 1320);
    OFFSET(Fsr212::FfxFsr2DispatchDescription, reset, 1336);
    OFFSET(Fsr212::FfxFsr2DispatchDescription, cameraFovAngleVertical, 1348);
    expect("FfxFsr2DispatchDescription size", sizeof(Fsr212::FfxFsr2DispatchDescription), 1352);
    OFFSET(Fsr212::FfxResource, name, 8);
    OFFSET(Fsr212::FfxResource, description, 136);
    OFFSET(Fsr212::FfxResource, state, 164);
    OFFSET(Fsr212::FfxResource, isDepth, 168);
#endif
    if (sets == 0) {
        std::printf("no vendor headers found; nothing was checked\n");
        return 77;
    }
    std::printf("vendor_census_layouts: %d header set(s) checked, %d failure(s)\n", sets, g_failures);
    return g_failures ? 1 : 0;
}
