// Small ABI boundary for the signed AMD FidelityFX SDK 1.1.4 Vulkan runtime.
// The SDK headers and binary are supplied from the sdk/fidelityfx-vk submodule at build time.
// owner.rs owns the runtime binding and the SR, interpolation and swapchain
// owners; the leaves below keep the typed FidelityFX and Vulkan calls and the
// presentation callback shell.
#include <windows.h>
#include <cstdint>
#include <cstddef>
#include <cmath>
#include <new>
#include <cstring>
#include <cstdio>
#include "ffx_api/ffx_api.h"
#include "ffx_api/ffx_upscale.h"
#include "ffx_api/ffx_framegeneration.h"
#include "ffx_api/vk/ffx_api_vk.h"
#include "bridge_platform.h"

/// @brief Owns one FidelityFX reconstruction effect and its creation descriptors.
/// @details The Rust SR context owns this capsule. Device handles and the process-retained SDK function table are borrowed. The host retires referencing Vulkan work before destruction.
struct rsf_fsr3sr_native {
    ffxContext effect = nullptr;
    ffxCreateContextDescUpscale upscale{};
    ffxCreateBackendVKDesc backend{};
    uint32_t width = 0, height = 0;
};

/// @brief Owns one FidelityFX interpolation effect, its creation descriptors and its display extent.
/// @details The Rust interpolation owner owns this capsule and its swapchain pairing. Proxy mode registers this capsule as the presentation callback context, so it lives until checked disable/drain and SDK release succeed.
struct rsf_fsr3fg_native {
    ffxContext effect = nullptr;
    ffxCreateContextDescFrameGeneration frameGeneration{};
    ffxCreateContextDescFrameGenerationHudless hudlessFormat{};
    ffxCreateBackendVKDesc backend{};
    uint32_t width = 0, height = 0;
};

// FFX owns all five Vulkan swapchain entry points while its proxy is active.
// Keep the queried function table and context together so callers cannot pair
// native acquire/present with images returned by the proxy.
/// @brief Owns a FidelityFX swapchain context and its complete replacement Vulkan function table.
/// @details The Rust swapchain owner owns this capsule with the device, current chain and activeFg pairing.
struct rsf_fsr3wsi_native {
    ffxContext context = nullptr;
    ffxQueryDescSwapchainReplacementFunctionsVK functions{};
};

static PFN_vkGetDeviceProcAddr vulkanGetDeviceProcAddr;

/// @brief Returns the SDK table that VulkanStoryFsr3Open bound in Rust.
/// @details All entries are null before a successful open and never change afterwards. Only the presentation callback reads it; Rust passes each other leaf its entry.
static const rsf_fsr3_sdk& Sdk() { return *rsf_core_fsr3_sdk(); }

/// @brief Resolves Vulkan device commands with the retained promoted-core fallback for the SDK's KHR buffer-requirements query.
/// @details The returned function pointer borrows the Vulkan loader/device lifetime.
static PFN_vkVoidFunction VKAPI_PTR Fsr3GetDeviceProcAddr(VkDevice device, const char* name) {
    auto function = vulkanGetDeviceProcAddr(device, name);
    // The SDK 1.1.4 Vulkan backend calls this KHR alias unconditionally. Vulkan
    // 1.1+ devices may expose only the promoted core entry point.
    if (!function && std::strcmp(name, "vkGetBufferMemoryRequirements2KHR") == 0)
        function = vulkanGetDeviceProcAddr(device, "vkGetBufferMemoryRequirements2");
    return function;
}

/// @brief Rebinds the shared vkGetDeviceProcAddr from the already loaded vulkan-1.dll.
/// @return Whether the loader export resolved.
static bool BindVulkanLoader() {
    HMODULE vulkan = GetModuleHandleW(L"vulkan-1.dll");
    vulkanGetDeviceProcAddr = vulkan ? reinterpret_cast<PFN_vkGetDeviceProcAddr>(
        GetProcAddress(vulkan, "vkGetDeviceProcAddr")) : nullptr;
    return vulkanGetDeviceProcAddr != nullptr;
}

/// @brief Converts borrowed Vulkan image metadata into the SDK resource description/state convention.
/// @details Input images are described as COMPUTE_READ, outputs as UNORDERED_ACCESS. This conversion records no barrier; the host must establish the matching Vulkan usage before dispatch. Unlisted formats become UNKNOWN.
static FfxApiResource Resource(VulkanStoryFsr3Image image, bool output = false) {
    FfxApiResource resource{};
    resource.resource = reinterpret_cast<void*>(image.image);
    resource.description.type = FFX_API_RESOURCE_TYPE_TEXTURE2D;
    resource.description.width = image.width;
    resource.description.height = image.height;
    resource.description.depth = 1;
    resource.description.mipCount = 1;
    resource.description.usage = output ? FFX_API_RESOURCE_USAGE_UAV :
        image.format == VK_FORMAT_D32_SFLOAT ? FFX_API_RESOURCE_USAGE_DEPTHTARGET :
        FFX_API_RESOURCE_USAGE_READ_ONLY;
    resource.state = output ? FFX_API_RESOURCE_STATE_UNORDERED_ACCESS : FFX_API_RESOURCE_STATE_COMPUTE_READ;
    switch (image.format) {
    case VK_FORMAT_R16G16B16A16_SFLOAT: resource.description.format = FFX_API_SURFACE_FORMAT_R16G16B16A16_FLOAT; break;
    case VK_FORMAT_R16G16_SFLOAT: resource.description.format = FFX_API_SURFACE_FORMAT_R16G16_FLOAT; break;
    case VK_FORMAT_D32_SFLOAT: resource.description.format = FFX_API_SURFACE_FORMAT_R32_FLOAT; break;
    case VK_FORMAT_R8G8B8A8_UNORM: resource.description.format = FFX_API_SURFACE_FORMAT_R8G8B8A8_UNORM; break;
    case VK_FORMAT_R8_UNORM: resource.description.format = FFX_API_SURFACE_FORMAT_R8_UNORM; break;
    default: resource.description.format = FFX_API_SURFACE_FORMAT_UNKNOWN; break;
    }
    return resource;
}

/// @brief Resolves one SDK export from the borrowed runtime module.
static void* SrResolve(void* module, const char* name) {
    return reinterpret_cast<void*>(GetProcAddress(static_cast<HMODULE>(module), name));
}

static rsf_fsr3sr_native* SrAllocate() { return new (std::nothrow) rsf_fsr3sr_native(); }

static void SrRelease(rsf_fsr3sr_native* value) { delete value; }

/// @brief Binds the Vulkan loader and probes the KHR buffer-requirements entry with its promoted-core fallback.
static uint32_t SrBindVulkan(VkDevice device) {
    if (!BindVulkanLoader()) return 0;
    return RSF_FSR3SR_VULKAN_LOADER |
        (Fsr3GetDeviceProcAddr(device, "vkGetBufferMemoryRequirements2KHR") ? RSF_FSR3SR_VULKAN_BUFFER_REQUIREMENTS : 0u);
}

/// @brief Fills the HDR/auto-exposure upscale and Vulkan backend descriptors and creates the effect.
/// @details Rust has validated the request and bound the Vulkan loader.
static int32_t SrCreate(rsf_fsr3sr_native* value, PfnFfxCreateContext create, const rsf_fsr3sr_create* request) {
    value->width = request->width;
    value->height = request->height;
    value->upscale.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE;
    value->upscale.header.pNext = &value->backend.header;
    value->upscale.flags = FFX_UPSCALE_ENABLE_HIGH_DYNAMIC_RANGE | FFX_UPSCALE_ENABLE_AUTO_EXPOSURE;
    value->upscale.maxRenderSize = {request->render_width, request->render_height};
    value->upscale.maxUpscaleSize = {request->width, request->height};
    value->backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK;
    value->backend.vkDevice = request->device;
    value->backend.vkPhysicalDevice = request->physical;
    value->backend.vkDeviceProcAddr = Fsr3GetDeviceProcAddr;
    return static_cast<int32_t>(create(&value->effect, &value->upscale.header, nullptr));
}

/// @brief Queries input dimensions for the requested output dimensions and FidelityFX quality mode.
static int32_t SrPlan(PfnFfxQuery query, uint32_t displayWidth, uint32_t displayHeight, uint32_t quality,
    uint32_t* renderWidth, uint32_t* renderHeight) {
    ffxQueryDescUpscaleGetRenderResolutionFromQualityMode desc{};
    desc.header.type = FFX_API_QUERY_DESC_TYPE_UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE;
    desc.displayWidth = displayWidth;
    desc.displayHeight = displayHeight;
    desc.qualityMode = quality;
    desc.pOutRenderWidth = renderWidth;
    desc.pOutRenderHeight = renderHeight;
    return static_cast<int32_t>(query(nullptr, &desc.header));
}

/// @brief Records reconstruction with independent R8 reactive and composition masks.
/// @details Rust has checked the six formats and chosen the frame time. The call records GPU work without submitting or waiting.
static int32_t SrEvaluate(rsf_fsr3sr_native* context, PfnFfxDispatch dispatch,
    const VulkanStoryFsr3Frame* frame, float frameTimeMs) {
    ffxDispatchDescUpscale desc{};
    desc.header.type = FFX_API_DISPATCH_DESC_TYPE_UPSCALE;
    desc.commandList = frame->commands;
    desc.color = Resource(frame->color);
    desc.depth = Resource(frame->depth);
    desc.motionVectors = Resource(frame->motion);
    desc.reactive = Resource(frame->reactive);
    desc.transparencyAndComposition = Resource(frame->composition);
    desc.output = Resource(frame->output, true);
    desc.jitterOffset = {frame->jitterX, frame->jitterY};
    desc.motionVectorScale = {1.0f, 1.0f};
    desc.renderSize = {frame->color.width, frame->color.height};
    desc.upscaleSize = {frame->output.width, frame->output.height};
    desc.frameTimeDelta = frameTimeMs;
    desc.preExposure = 1.0f;
    desc.reset = frame->reset != 0;
    desc.cameraNear = frame->nearPlane;
    desc.cameraFar = frame->farPlane;
    desc.cameraFovAngleVertical = frame->fovRadians;
    desc.viewSpaceToMetersFactor = 1.0f;
    return static_cast<int32_t>(dispatch(&context->effect, &desc.header));
}

static uint32_t SrHasEffect(const rsf_fsr3sr_native* context) { return context->effect ? 1u : 0u; }

/// @brief Releases the SDK effect; a nonzero result leaves the capsule attached.
static int32_t SrDestroy(rsf_fsr3sr_native* context, PfnFfxDestroyContext destroy) {
    return static_cast<int32_t>(destroy(&context->effect, nullptr));
}

static const rsf_fsr3sr_platform srPlatform = {
    sizeof(rsf_fsr3sr_platform), RSF_FSR3SR_PLATFORM_ABI_VERSION,
    SrResolve, SrAllocate, SrRelease, SrBindVulkan, SrCreate, SrPlan, SrEvaluate, SrHasEffect, SrDestroy,
};

static rsf_fsr3wsi_native* WsiAllocate() { return new (std::nothrow) rsf_fsr3wsi_native(); }

static void WsiRelease(rsf_fsr3wsi_native* value) { delete value; }

/// @brief Creates the SDK presentation context with four queue handles in one family.
/// @details Rust has validated the request. The SDK writes the created chain through the Rust owner's chain slot.
static int32_t WsiCreate(rsf_fsr3wsi_native* value, PfnFfxCreateContext create,
    const rsf_fsr3wsi_create* request, VkSwapchainKHR* chain) {
    ffxCreateContextDescFrameGenerationSwapChainVK desc{};
    desc.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FGSWAPCHAIN_VK;
    desc.physicalDevice = request->physical;
    desc.device = request->device;
    desc.swapchain = chain;
    desc.createInfo = *request->info;
    desc.createInfo.oldSwapchain = VK_NULL_HANDLE;
    // The FFX Vulkan proxy requires four distinct VkQueue handles even when
    // async frame-generation workloads are disabled. They can share one family.
    desc.gameQueue = {request->game, request->queue_family, nullptr};
    desc.asyncComputeQueue = {request->async, request->queue_family, nullptr};
    desc.presentQueue = {request->present, request->queue_family, nullptr};
    desc.imageAcquireQueue = {request->acquire, request->queue_family, nullptr};
    return static_cast<int32_t>(create(&value->context, &desc.header, nullptr));
}

/// @brief Queries the complete replacement Vulkan function table for the created context.
static int32_t WsiQuery(rsf_fsr3wsi_native* value, PfnFfxQuery query) {
    value->functions.header.type = FFX_API_QUERY_DESC_TYPE_FGSWAPCHAIN_FUNCTIONS_VK;
    return static_cast<int32_t>(query(&value->context, &value->functions.header));
}

static uint32_t WsiComplete(const rsf_fsr3wsi_native* value) {
    return value->functions.pOutCreateSwapchainFFXAPI && value->functions.pOutDestroySwapchainFFXAPI &&
        value->functions.pOutGetSwapchainImagesKHR && value->functions.pOutAcquireNextImageKHR &&
        value->functions.pOutQueuePresentKHR ? 1u : 0u;
}

static uint32_t WsiHasContext(const rsf_fsr3wsi_native* value) { return value->context ? 1u : 0u; }

/// @brief Releases the SDK presentation context; a nonzero result leaves the capsule attached.
static int32_t WsiDestroy(rsf_fsr3wsi_native* value, PfnFfxDestroyContext destroy) {
    return static_cast<int32_t>(destroy(&value->context, nullptr));
}

/// @brief Waits for the proxy's queued presents.
static int32_t WsiWait(rsf_fsr3wsi_native* value, PfnFfxDispatch dispatch) {
    ffxDispatchDescFrameGenerationSwapChainWaitForPresentsVK wait{};
    wait.header.type = FFX_API_DISPATCH_DESC_TYPE_FGSWAPCHAIN_WAIT_FOR_PRESENTS_VK;
    return static_cast<int32_t>(dispatch(&value->context, &wait.header));
}

/// @brief Registers premultiplied UI with internal SDK double buffering, or clears the registration for a null frame.
static int32_t WsiRegisterUi(rsf_fsr3wsi_native* value, PfnFfxConfigure configure, const VulkanStoryFsr3FgFrame* frame) {
    ffxConfigureDescFrameGenerationSwapChainRegisterUiResourceVK ui{};
    ui.header.type = FFX_API_CONFIGURE_DESC_TYPE_FGSWAPCHAIN_REGISTERUIRESOURCE_VK;
    if (frame) {
        ui.uiResource = Resource(frame->ui);
        ui.flags = FFX_FRAMEGENERATION_UI_COMPOSITION_FLAG_USE_PREMUL_ALPHA |
            FFX_FRAMEGENERATION_UI_COMPOSITION_FLAG_ENABLE_INTERNAL_UI_DOUBLE_BUFFERING;
    }
    return static_cast<int32_t>(configure(&value->context, &ui.header));
}

static VkResult WsiRecreate(rsf_fsr3wsi_native* value, VkDevice device, const VkSwapchainCreateInfoKHR* info,
    VkSwapchainKHR* chain) {
    return value->functions.pOutCreateSwapchainFFXAPI(device, info, nullptr, chain, value->context);
}

static VkResult WsiGetImages(rsf_fsr3wsi_native* value, VkDevice device, VkSwapchainKHR chain,
    uint32_t* count, VkImage* images) {
    return value->functions.pOutGetSwapchainImagesKHR(device, chain, count, images);
}

static VkResult WsiAcquire(rsf_fsr3wsi_native* value, VkDevice device, VkSwapchainKHR chain, uint64_t timeout,
    VkSemaphore semaphore, VkFence fence, uint32_t* imageIndex) {
    return value->functions.pOutAcquireNextImageKHR(device, chain, timeout, semaphore, fence, imageIndex);
}

static VkResult WsiPresent(rsf_fsr3wsi_native* value, VkQueue queue, const VkPresentInfoKHR* info) {
    return value->functions.pOutQueuePresentKHR(queue, info);
}

static int32_t WsiDestroyChain(rsf_fsr3wsi_native* value, VkDevice device, VkSwapchainKHR chain) {
    if (!value->functions.pOutDestroySwapchainFFXAPI) return -1;
    value->functions.pOutDestroySwapchainFFXAPI(device, chain, nullptr, value->context);
    return 0;
}

static uint64_t WsiLastPresentCount(rsf_fsr3wsi_native* value, VkSwapchainKHR chain) {
    return value->functions.pOutGetLastPresentCountFFXAPI ? value->functions.pOutGetLastPresentCountFFXAPI(chain) : 0;
}

static rsf_fsr3fg_native* FgAllocate() { return new (std::nothrow) rsf_fsr3fg_native(); }

static void FgRelease(rsf_fsr3fg_native* value) { delete value; }

static uint32_t FgBindVulkan() { return BindVulkanLoader() ? 1u : 0u; }

/// @brief Creates a Vulkan interpolation effect with explicit display extent and HUD-free RGBA format metadata.
/// @details The selected backbuffer format can be BGRA while the renderer's upright HUD-free source remains RGBA. Rust has validated the request and bound the Vulkan loader.
static int32_t FgCreate(rsf_fsr3fg_native* value, PfnFfxCreateContext create, const rsf_fsr3fg_create* request) {
    value->width = request->width;
    value->height = request->height;
    value->frameGeneration.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION;
    value->frameGeneration.header.pNext = &value->hudlessFormat.header;
    value->frameGeneration.displaySize = {request->width, request->height};
    value->frameGeneration.maxRenderSize = {request->width, request->height};
    value->frameGeneration.backBufferFormat = request->back_buffer_format == VK_FORMAT_B8G8R8A8_UNORM ?
        FFX_API_SURFACE_FORMAT_B8G8R8A8_UNORM : FFX_API_SURFACE_FORMAT_R8G8B8A8_UNORM;
    // The proxy's real backbuffer may be BGRA, while the Vulkan renderer's
    // upright HUD-less image is RGBA. FFX keeps a previous HUD-less source in
    // this format; without the linked descriptor it assumes backBufferFormat.
    value->hudlessFormat.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION_HUDLESS;
    value->hudlessFormat.header.pNext = &value->backend.header;
    value->hudlessFormat.hudlessBackBufferFormat = FFX_API_SURFACE_FORMAT_R8G8B8A8_UNORM;
    value->backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK;
    value->backend.vkDevice = request->device;
    value->backend.vkPhysicalDevice = request->physical;
    value->backend.vkDeviceProcAddr = Fsr3GetDeviceProcAddr;
    return static_cast<int32_t>(create(&value->effect, &value->frameGeneration.header, nullptr));
}

static uint32_t FgHasEffect(const rsf_fsr3fg_native* value) { return value->effect ? 1u : 0u; }

/// @brief Releases the interpolation effect; a nonzero result leaves the capsule attached.
static int32_t FgDestroy(rsf_fsr3fg_native* value, PfnFfxDestroyContext destroy) {
    return static_cast<int32_t>(destroy(&value->effect, nullptr));
}

/// @brief SDK presentation callback that dispatches interpolation through its borrowed effect capsule.
/// @details The capsule must remain live until proxy presentation has drained.
/// @return FidelityFX dispatch result, or its parameter error for a missing capsule/description/function.
static ffxReturnCode_t Fsr3GenerateOnPresent(ffxDispatchDescFrameGeneration* params, void* user) {
    auto* context = static_cast<rsf_fsr3fg_native*>(user);
    return context && params && Sdk().dispatch ?
        Sdk().dispatch(&context->effect, &params->header) : FFX_API_RETURN_ERROR_PARAMETER;
}

/// @brief Configures interpolation off for the linked chain.
static int32_t FgDisable(rsf_fsr3fg_native* value, PfnFfxConfigure configure, VkSwapchainKHR chain) {
    ffxConfigureDescFrameGeneration config{};
    config.header.type = FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION;
    config.swapChain = reinterpret_cast<void*>(chain);
    config.frameGenerationEnabled = false;
    return static_cast<int32_t>(configure(&value->effect, &config.header));
}

static FfxApiRect2D GenerationRect(const rsf_fsr3fg_native* value) {
    return {0, 0, static_cast<int32_t>(value->width), static_cast<int32_t>(value->height)};
}

/// @brief Configures interpolation on; a null chain selects direct interpolation, otherwise the proxy callback.
static int32_t FgConfigure(rsf_fsr3fg_native* value, PfnFfxConfigure configure,
    const VulkanStoryFsr3FgFrame* frame, VkSwapchainKHR chain) {
    ffxConfigureDescFrameGeneration config{};
    config.header.type = FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION;
    // The interpolation effect is independent of the SDK's multi-queue
    // swapchain proxy. A host-owned presentation path can dispatch it on its
    // existing command buffer without fabricating additional VkQueue handles.
    if (!chain) {
        config.flags = FFX_FRAMEGENERATION_FLAG_NO_SWAPCHAIN_CONTEXT_NOTIFY;
    } else {
        config.swapChain = reinterpret_cast<void*>(chain);
        config.frameGenerationCallback = Fsr3GenerateOnPresent;
        config.frameGenerationCallbackUserContext = value;
    }
    config.frameGenerationEnabled = true;
    config.HUDLessColor = frame->hudless.image ? Resource(frame->hudless) : FfxApiResource{};
    config.generationRect = GenerationRect(value);
    config.frameID = frame->frameId;
    return static_cast<int32_t>(configure(&value->effect, &config.header));
}

/// @brief Records depth/motion/camera preparation. Motion arrives in upright space and its vertical scale is negated here.
static int32_t FgPrepare(rsf_fsr3fg_native* value, PfnFfxDispatch dispatch,
    const VulkanStoryFsr3FgFrame* frame, float frameTimeMs) {
    ffxDispatchDescFrameGenerationPrepare prepare{};
    prepare.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE;
    ffxDispatchDescFrameGenerationPrepareCameraInfo camera{};
    camera.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_CAMERAINFO;
    camera.cameraPosition[0] = frame->cameraPos[0];
    camera.cameraPosition[1] = frame->cameraPos[1];
    camera.cameraPosition[2] = frame->cameraPos[2];
    camera.cameraUp[0] = frame->cameraUp[0];
    camera.cameraUp[1] = frame->cameraUp[1];
    camera.cameraUp[2] = frame->cameraUp[2];
    camera.cameraRight[0] = frame->cameraRight[0];
    camera.cameraRight[1] = frame->cameraRight[1];
    camera.cameraRight[2] = frame->cameraRight[2];
    camera.cameraForward[0] = frame->cameraForward[0];
    camera.cameraForward[1] = frame->cameraForward[1];
    camera.cameraForward[2] = frame->cameraForward[2];
    prepare.header.pNext = &camera.header;
    prepare.frameID = frame->frameId;
    prepare.commandList = frame->commands;
    prepare.renderSize = {frame->depth.width, frame->depth.height};
    prepare.jitterOffset = {frame->jitterX, frame->jitterY};
    // The Vulkan bridge flips the GL-oriented motion image before this call.
    // Negate its vertical component to keep vectors in upright present space.
    prepare.motionVectorScale = {1.0f, -1.0f};
    prepare.frameTimeDelta = frameTimeMs;
    prepare.cameraNear = frame->nearPlane;
    prepare.cameraFar = frame->farPlane;
    prepare.cameraFovAngleVertical = frame->fovRadians;
    prepare.viewSpaceToMetersFactor = 1.0f;
    prepare.depth = Resource(frame->depth);
    prepare.motionVectors = Resource(frame->motion);
    return static_cast<int32_t>(dispatch(&value->effect, &prepare.header));
}

/// @brief Records exactly one generated output on the host command buffer.
static int32_t FgGenerate(rsf_fsr3fg_native* value, PfnFfxDispatch dispatch, const VulkanStoryFsr3FgFrame* frame) {
    ffxDispatchDescFrameGeneration generate{};
    generate.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION;
    generate.commandList = frame->commands;
    generate.presentColor = Resource(frame->color);
    generate.outputs[0] = Resource(frame->output, true);
    generate.numGeneratedFrames = 1;
    generate.reset = frame->reset != 0;
    generate.backbufferTransferFunction = FFX_API_BACKBUFFER_TRANSFER_FUNCTION_SRGB;
    generate.minMaxLuminance[0] = 0.0f;
    generate.minMaxLuminance[1] = 1.0f;
    generate.generationRect = GenerationRect(value);
    generate.frameID = frame->frameId;
    return static_cast<int32_t>(dispatch(&value->effect, &generate.header));
}

static const rsf_fsr3fg_platform fgPlatform = {
    sizeof(rsf_fsr3fg_platform), RSF_FSR3FG_PLATFORM_ABI_VERSION,
    WsiAllocate, WsiRelease, WsiCreate, WsiQuery, WsiComplete, WsiHasContext, WsiDestroy, WsiWait,
    WsiRegisterUi, WsiRecreate, WsiGetImages, WsiAcquire, WsiPresent, WsiDestroyChain, WsiLastPresentCount,
    FgAllocate, FgRelease, FgBindVulkan, FgCreate, FgHasEffect, FgDestroy, FgDisable, FgConfigure,
    FgPrepare, FgGenerate,
};

/// @brief Binds the required FidelityFX API exports from a borrowed signed-runtime module.
/// @details One runtime identity is retained for this bridge. The caller keeps the module loaded for all contexts and deferred destruction.
/// @param runtime Borrowed SDK module; this bridge does not acquire or release a module reference.
/// @return Zero on success or rebinding the same module, -1 for null, -2 for another runtime or missing exports.
extern "C" __declspec(dllexport) int VulkanStoryFsr3Open(HMODULE runtime) {
    return rsf_core_fsr3_open(&srPlatform, runtime);
}

/// @brief Queries input dimensions for the requested output dimensions and FidelityFX quality mode.
/// @param displayWidth Requested output width.
/// @param displayHeight Requested output height.
/// @param quality FidelityFX quality-mode value.
/// @param renderWidth Required SDK output slot for input width.
/// @param renderHeight Required SDK output slot for input height.
/// @return SDK query result; -1 when the query function or output pointers are absent.
extern "C" __declspec(dllexport) int VulkanStoryFsr3Plan(
    uint32_t displayWidth, uint32_t displayHeight, uint32_t quality,
    uint32_t* renderWidth, uint32_t* renderHeight) {
    return rsf_core_fsr3_plan(&srPlatform, displayWidth, displayHeight, quality, renderWidth, renderHeight);
}

/// @brief Creates an HDR/auto-exposure reconstruction context with independent input and output maxima.
/// @details The context borrows the selected Vulkan device/physical device and the bridge's process-retained SDK table.
/// @param device Borrowed live logical device.
/// @param physical Borrowed matching physical device.
/// @param renderWidth Nonzero maximum input width.
/// @param renderHeight Nonzero maximum input height.
/// @param width Nonzero maximum display/output width.
/// @param height Nonzero maximum display/output height.
/// @param result Required owner slot written on success; callers initialize it before calling.
/// @return Zero on success, -1 for missing prerequisites, -2 for allocation failure, -3 for required Vulkan dispatch absence, or the SDK creation result.
extern "C" __declspec(dllexport) int VulkanStoryFsr3Create(
    VkDevice device, VkPhysicalDevice physical, uint32_t renderWidth, uint32_t renderHeight,
    uint32_t width, uint32_t height,
    VulkanStoryFsr3Context** result) {
    const rsf_fsr3sr_create request{device, physical, renderWidth, renderHeight, width, height};
    return rsf_core_fsr3_create(&srPlatform, &request, result);
}

/// @brief Records reconstruction using matching temporal constants and independent R8 reactive/composition masks.
/// @details Inputs must already have SDK-compatible compute-read usage and output storage-write usage. The call records GPU work without submitting or waiting; all borrowed images remain live through host timeline completion.
/// @param context Borrowed live SR owner.
/// @param frame Borrowed 184-byte frame record with render-resolution material/composition masks and matching input/output extents.
/// @return SDK dispatch result; -1 for absent context/frame/dispatch, -2 for incompatible required image formats.
extern "C" __declspec(dllexport) int VulkanStoryFsr3EvaluateWithComposition(
    VulkanStoryFsr3Context* context, const VulkanStoryFsr3Frame* frame) {
    return rsf_core_fsr3_evaluate(context, frame);
}

/// @brief Destroys the reconstruction effect then deletes its owner on successful SDK release.
/// @details The caller must first complete Vulkan work referencing the effect. A nonzero SDK release result preserves the context pointer.
/// @return Zero for successful destruction/null, -1 when an existing effect has no destroy export, or the SDK release error.
extern "C" __declspec(dllexport) int VulkanStoryFsr3Destroy(VulkanStoryFsr3Context* context) {
    return rsf_core_fsr3_destroy(context);
}

/// @brief Creates the FidelityFX Vulkan presentation context and queries its complete replacement function table.
/// @details The current SDK path requires four distinct queue handles in the supplied family. No native/proxy operation mixing is permitted while this owner is active.
/// @param physical Borrowed selected physical device.
/// @param device Borrowed matching logical device.
/// @param gameQueue Borrowed renderer graphics queue.
/// @param asyncQueue Borrowed distinct asynchronous-compute queue.
/// @param presentQueue Borrowed distinct proxy present queue.
/// @param acquireQueue Borrowed distinct proxy acquisition queue.
/// @param queueFamily Shared family index used by all four queues.
/// @param info Borrowed creation information; initial oldSwapchain must be null.
/// @param result Required native owner output, cleared before creation.
/// @return Zero on success; -1 for prerequisites, -2 for allocation, -3 for incomplete replacement functions, or an SDK result.
// A failed creation can return a cleanup-only owner if SDK rollback failed.
// Retry VulkanStoryFsr3SwapchainDestroy on that owner before releasing the device.
extern "C" __declspec(dllexport) int VulkanStoryFsr3SwapchainCreate(
    VkPhysicalDevice physical, VkDevice device, VkQueue gameQueue, VkQueue asyncQueue,
    VkQueue presentQueue, VkQueue acquireQueue, uint32_t queueFamily,
    const VkSwapchainCreateInfoKHR* info, VulkanStoryFsr3Swapchain** result) {
    const rsf_fsr3wsi_create request{physical, device, gameQueue, asyncQueue, presentQueue, acquireQueue, info, queueFamily};
    return rsf_core_fsr3_swapchain_create(&fgPlatform, &request, result);
}

/// @brief Returns the chain retained by the supplied SDK presentation owner.
/// @details The handle is borrowed and must be acquired/presented/destroyed through this owner.
/// @return Current chain, or VK_NULL_HANDLE for a null owner.
extern "C" __declspec(dllexport) VkSwapchainKHR VulkanStoryFsr3SwapchainHandle(
    VulkanStoryFsr3Swapchain* value) { return rsf_core_fsr3_swapchain_handle(value); }

/// @brief Creates a replacement chain through the existing SDK presentation context.
/// @details The owner's current-chain handle updates only on VK_SUCCESS.
/// @return Proxy Vulkan creation result, or VK_ERROR_INITIALIZATION_FAILED for missing required arguments.
extern "C" __declspec(dllexport) VkResult VulkanStoryFsr3SwapchainRecreate(
    VulkanStoryFsr3Swapchain* value, const VkSwapchainCreateInfoKHR* info,
    VkSwapchainKHR* chain) {
    return rsf_core_fsr3_swapchain_recreate(value, info, chain);
}

/// @brief Enumerates proxy-owned swapchain images with Vulkan count/query semantics.
/// @details Returned VkImages borrow chain lifetime and are consumed through the same presentation owner.
/// @return Proxy Vulkan result, or VK_ERROR_INITIALIZATION_FAILED for an absent owner/count pointer.
extern "C" __declspec(dllexport) VkResult VulkanStoryFsr3SwapchainGetImages(
    VulkanStoryFsr3Swapchain* value, VkSwapchainKHR chain, uint32_t* count, VkImage* images) {
    return rsf_core_fsr3_swapchain_get_images(value, chain, count, images);
}

/// @brief Acquires an image through the SDK replacement entry point.
/// @details Timeout, binary semaphore/fence and output index follow the borrowed Vulkan acquisition contract.
/// @return Proxy Vulkan result, or VK_ERROR_INITIALIZATION_FAILED for an absent owner/index pointer.
extern "C" __declspec(dllexport) VkResult VulkanStoryFsr3SwapchainAcquire(
    VulkanStoryFsr3Swapchain* value, VkSwapchainKHR chain, uint64_t timeout,
    VkSemaphore semaphore, VkFence fence, uint32_t* imageIndex) {
    return rsf_core_fsr3_swapchain_acquire(value, chain, timeout, semaphore, fence, imageIndex);
}

/// @brief Queues presentation through the SDK replacement dispatch.
/// @details Success does not establish GPU completion or scanout completion; presentation retirement uses the SDK drain operation.
/// @return Proxy Vulkan result, or VK_ERROR_INITIALIZATION_FAILED for an absent owner/present description.
extern "C" __declspec(dllexport) VkResult VulkanStoryFsr3SwapchainPresent(
    VulkanStoryFsr3Swapchain* value, VkQueue queue, const VkPresentInfoKHR* info) {
    return rsf_core_fsr3_swapchain_present(value, queue, info);
}

/// @brief Disables linked interpolation for this chain and waits for SDK presentation work.
/// @details A failed disable/drain prevents the caller from destroying the chain or retiring tagged inputs.
/// @return Zero for a null chain or successful preparation, -1 for missing prerequisites, otherwise the SDK failure.
extern "C" __declspec(dllexport) int VulkanStoryFsr3SwapchainPrepareDestroy(
    VulkanStoryFsr3Swapchain* value, VkSwapchainKHR chain) {
    return rsf_core_fsr3_swapchain_prepare_destroy(value, chain);
}

/// @brief Prepares release and destroys the specified chain through its SDK owner.
/// @details Checked preparation failure preserves the chain/ownership association. Successful release clears the current-chain field only when it matches.
/// @return Zero on success/null chain, -1 for missing destruction dispatch, or the preparation error.
extern "C" __declspec(dllexport) int VulkanStoryFsr3SwapchainDestroyChainChecked(
    VulkanStoryFsr3Swapchain* value, VkSwapchainKHR chain) {
    return rsf_core_fsr3_swapchain_destroy_chain(value, chain);
}

// Preserve the old export's signature. New managed callers require the checked
// export; legacy callers at least leave failed SDK owners intact.
/// @brief Legacy void chain destruction that logs failed checked preparation while retaining ownership.
/// @details New callers use DestroyChainChecked to observe the release result.
extern "C" __declspec(dllexport) void VulkanStoryFsr3SwapchainDestroyChain(
    VulkanStoryFsr3Swapchain* value, VkSwapchainKHR chain) {
    int code = rsf_core_fsr3_swapchain_destroy_chain(value, chain);
    if (code != 0)
        std::fprintf(stderr, "[FidelityFX] swapchain destruction retained ownership after error %d\n", code);
}

/// @brief Queries the SDK's actual presentation count for the supplied proxy chain.
/// @return SDK count, or zero for missing owner/chain/count function.
extern "C" __declspec(dllexport) uint64_t VulkanStoryFsr3SwapchainLastPresentCount(
    VulkanStoryFsr3Swapchain* value, VkSwapchainKHR chain) {
    return rsf_core_fsr3_swapchain_last_present_count(value, chain);
}

/// @brief Disables linked interpolation, drains presentation and releases the SDK presentation context.
/// @details Failure before SDK destruction completion leaves the native owner attached. The host retains the Vulkan device/queues until this owner is released.
/// @return Zero on successful destruction/null, -1 for a missing required export, or disable/drain/destruction error.
extern "C" __declspec(dllexport) int VulkanStoryFsr3SwapchainDestroy(
    VulkanStoryFsr3Swapchain* value) {
    return rsf_core_fsr3_swapchain_destroy(value);
}

/// @brief Creates a Vulkan interpolation effect with explicit display extent and HUD-free RGBA format metadata.
/// @details The selected backbuffer format can be BGRA while the renderer's upright HUD-free source remains RGBA. Device/physical-device handles are borrowed.
/// @return Zero on success, -1 for prerequisites, -2 for allocation failure, -3 for Vulkan dispatch absence, or the SDK creation result.
extern "C" __declspec(dllexport) int VulkanStoryFsr3FgCreate(
    VkDevice device, VkPhysicalDevice physical, uint32_t width, uint32_t height, VkFormat backBufferFormat,
    VulkanStoryFsr3FgContext** result) {
    const rsf_fsr3fg_create request{device, physical, width, height, backBufferFormat};
    return rsf_core_fsr3_fg_create(&fgPlatform, &request, result);
}

/// @brief Disables proxy interpolation, drains matching SDK presents and clears registered UI ownership.
/// @details The stored swapchain association is authoritative. Associations are cleared only after every SDK operation succeeds; on failure keep the effect and tagged inputs live.
/// @return Zero for an unassociated/null context or success, -1 for missing prerequisites, otherwise the SDK operation error.
extern "C" __declspec(dllexport) int VulkanStoryFsr3FgDisable(
    VulkanStoryFsr3FgContext* context, VulkanStoryFsr3Swapchain* swapchain) {
    return rsf_core_fsr3_fg_disable(context, swapchain);
}

/// @brief Configures interpolation and records matching depth/motion/camera preparation for the selected presentation path.
/// @details Direct mode records one generated output on the host command buffer. Proxy mode links the effect callback and registers premultiplied UI with internal SDK double buffering. Motion arrives in upright space and its vertical scale is negated here.
/// @param context Borrowed interpolation owner.
/// @param frame Borrowed matching frame resources/constants; null swapchainContext selects direct mode.
/// @return SDK configuration/dispatch result; -1 for missing prerequisites or -2 for unsupported resources/path metadata.
extern "C" __declspec(dllexport) int VulkanStoryFsr3FgEvaluate(
    VulkanStoryFsr3FgContext* context, const VulkanStoryFsr3FgFrame* frame) {
    return rsf_core_fsr3_fg_evaluate(context, frame);
}

// Explicit ABI capability prevents pairing a direct-present host with an older bridge.
/// @brief Requires the direct-interpolation ABI and delegates a frame with no swapchain owner.
/// @details This path records exactly one generated output for the host's own presentation sequence.
/// @return The shared evaluation result, or -1 for a null frame/nonnull swapchainContext.
extern "C" __declspec(dllexport) int VulkanStoryFsr3FgEvaluateDirect(
    VulkanStoryFsr3FgContext* context, const VulkanStoryFsr3FgFrame* frame) {
    return rsf_core_fsr3_fg_evaluate_direct(context, frame);
}

/// @brief Disables/drains any linked proxy effect and destroys the interpolation context.
/// @details The host must separately complete Vulkan work recorded by direct interpolation before calling. A nonzero disable or SDK destruction result retains this owner.
/// @return Zero on successful destruction/null, otherwise the disable/destruction result.
extern "C" __declspec(dllexport) int VulkanStoryFsr3FgDestroy(VulkanStoryFsr3FgContext* context) {
    return rsf_core_fsr3_fg_destroy(context);
}
