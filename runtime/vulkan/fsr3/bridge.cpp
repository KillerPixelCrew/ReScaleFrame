// Small ABI boundary for the signed AMD FidelityFX SDK 1.1.4 Vulkan runtime.
// The SDK headers and binary are supplied from the sdk/fidelityfx-vk submodule at build time.
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

/// @brief Borrowed level-zero Vulkan image metadata in the FidelityFX bridge C ABI.
/// @details The caller owns the VkImage and supplies its extent/format; this record owns no view, allocation or synchronization.
struct VulkanStoryFsr3Image {
    VkImage image;
    uint32_t width, height;
    VkFormat format;
};
/// @brief Fixed-layout SR dispatch record shared with the managed FSR3 ABI.
/// @details The 184-byte record contains six 24-byte image records; jitter begins at byte 152. Color/output use RGBA16F, depth D32F, motion RG16F and independent material reactive/composition masks R8_UNORM. The host keeps all resources live through GPU completion.
struct VulkanStoryFsr3Frame {
    VkCommandBuffer commands;
    /// @brief Borrowed SR scene color, depth, motion, output and separate render-resolution material/composition resources.
    VulkanStoryFsr3Image color, depth, motion, output, reactive, composition;
    float jitterX, jitterY, deltaMs, nearPlane, farPlane, fovRadians;
    uint32_t reset;
};
static_assert(sizeof(VulkanStoryFsr3Image) == 24);
static_assert(sizeof(VulkanStoryFsr3Frame) == 184);
static_assert(offsetof(VulkanStoryFsr3Frame, jitterX) == 152);
static_assert(offsetof(VulkanStoryFsr3Frame, composition) == 128);
/// @brief Owns one FidelityFX reconstruction effect and its creation descriptors.
/// @details Device handles and the process-retained SDK function table are borrowed. The host retires referencing Vulkan work before destruction.
struct VulkanStoryFsr3Context {
    ffxContext effect = nullptr;
    ffxCreateContextDescUpscale upscale{};
    ffxCreateBackendVKDesc backend{};
    uint32_t width = 0, height = 0;
};

/// @brief Fixed-layout interpolation record shared with the managed frame-generation ABI.
/// @details The 248-byte record supplies matching camera/timing resources. A null swapchainContext selects direct interpolation; a nonnull context selects the complete SDK swapchain owner.
struct VulkanStoryFsr3FgFrame {
    VkCommandBuffer commands;
    VulkanStoryFsr3Image color, depth, motion, output, hudless;
    float jitterX, jitterY, deltaMs, nearPlane, farPlane, fovRadians;
    uint64_t frameId;
    uint32_t reset;
    float cameraPos[3], cameraUp[3], cameraRight[3], cameraForward[3];
    VulkanStoryFsr3Image ui;
    /// @brief Borrowed SDK swapchain owner for proxy interpolation, or null for direct interpolation on the host command buffer.
    void* swapchainContext;
};
static_assert(sizeof(VulkanStoryFsr3FgFrame) == 248);

struct VulkanStoryFsr3Swapchain;
/// @brief Owns an interpolation effect and its borrowed association with an active SDK presentation context.
/// @details Proxy mode installs a callback referencing this object. Disable and drain presentation before retiring tagged inputs or destroying the effect.
struct VulkanStoryFsr3FgContext {
    ffxContext effect = nullptr;
    ffxCreateContextDescFrameGeneration frameGeneration{};
    ffxCreateContextDescFrameGenerationHudless hudlessFormat{};
    ffxCreateBackendVKDesc backend{};
    uint32_t width = 0, height = 0;
    VkSwapchainKHR chain = VK_NULL_HANDLE;
    VulkanStoryFsr3Swapchain* swapchain = nullptr;
};

// FFX owns all five Vulkan swapchain entry points while its proxy is active.
// Keep the queried function table and context together so callers cannot pair
// native acquire/present with images returned by the proxy.
/// @brief Owns a FidelityFX swapchain context and its complete replacement Vulkan function table.
/// @details Acquire, image enumeration, presentation and chain destruction must use this same owner. activeFg borrows the linked interpolation context until checked disable/drain succeeds.
struct VulkanStoryFsr3Swapchain {
    ffxContext context = nullptr;
    ffxQueryDescSwapchainReplacementFunctionsVK functions{};
    VkDevice device = VK_NULL_HANDLE;
    VkSwapchainKHR chain = VK_NULL_HANDLE;
    /// @brief Borrowed interpolation owner registered with this SDK swapchain; cleared only after successful disable/drain.
    VulkanStoryFsr3FgContext* activeFg = nullptr;
};

extern "C" __declspec(dllexport) int VulkanStoryFsr3FgDisable(
    VulkanStoryFsr3FgContext* context, VulkanStoryFsr3Swapchain* swapchain);

static HMODULE sdk;
static PfnFfxCreateContext createContext;
static PfnFfxDestroyContext destroyContext;
static PfnFfxQuery query;
static PfnFfxDispatch dispatch;
static PfnFfxConfigure configure;
static PFN_vkGetDeviceProcAddr vulkanGetDeviceProcAddr;

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

/// @brief Binds the required FidelityFX API exports from a borrowed signed-runtime module.
/// @details One runtime identity is retained for this bridge. The caller keeps the module loaded for all contexts and deferred destruction.
/// @param runtime Borrowed SDK module; this bridge does not acquire or release a module reference.
/// @return Zero on success or rebinding the same module, -1 for null, -2 for another runtime or missing exports.
extern "C" __declspec(dllexport) int VulkanStoryFsr3Open(HMODULE runtime) {
    if (!runtime) return -1;
    if (sdk) return sdk == runtime ? 0 : -2;
    auto create = reinterpret_cast<PfnFfxCreateContext>(GetProcAddress(runtime, "ffxCreateContext"));
    auto destroy = reinterpret_cast<PfnFfxDestroyContext>(GetProcAddress(runtime, "ffxDestroyContext"));
    auto queryFunction = reinterpret_cast<PfnFfxQuery>(GetProcAddress(runtime, "ffxQuery"));
    auto dispatchFunction = reinterpret_cast<PfnFfxDispatch>(GetProcAddress(runtime, "ffxDispatch"));
    auto configureFunction = reinterpret_cast<PfnFfxConfigure>(GetProcAddress(runtime, "ffxConfigure"));
    if (!create || !destroy || !queryFunction || !dispatchFunction || !configureFunction) return -2;
    sdk = runtime;
    createContext = create;
    destroyContext = destroy;
    query = queryFunction;
    dispatch = dispatchFunction;
    configure = configureFunction;
    return 0;
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
    if (!query || !renderWidth || !renderHeight) return -1;
    ffxQueryDescUpscaleGetRenderResolutionFromQualityMode desc{};
    desc.header.type = FFX_API_QUERY_DESC_TYPE_UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE;
    desc.displayWidth = displayWidth;
    desc.displayHeight = displayHeight;
    desc.qualityMode = quality;
    desc.pOutRenderWidth = renderWidth;
    desc.pOutRenderHeight = renderHeight;
    return static_cast<int>(query(nullptr, &desc.header));
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
    if (!createContext || !result || !device || !physical || !renderWidth || !renderHeight || !width || !height) return -1;
    auto* value = new (std::nothrow) VulkanStoryFsr3Context();
    if (!value) return -2;
    value->width = width;
    value->height = height;
    value->upscale.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE;
    value->upscale.header.pNext = &value->backend.header;
    value->upscale.flags = FFX_UPSCALE_ENABLE_HIGH_DYNAMIC_RANGE | FFX_UPSCALE_ENABLE_AUTO_EXPOSURE;
    value->upscale.maxRenderSize = {renderWidth, renderHeight};
    value->upscale.maxUpscaleSize = {width, height};
    value->backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK;
    value->backend.vkDevice = device;
    value->backend.vkPhysicalDevice = physical;
    HMODULE vulkan = GetModuleHandleW(L"vulkan-1.dll");
    vulkanGetDeviceProcAddr = vulkan ? reinterpret_cast<PFN_vkGetDeviceProcAddr>(
        GetProcAddress(vulkan, "vkGetDeviceProcAddr")) : nullptr;
    if (!vulkanGetDeviceProcAddr || !Fsr3GetDeviceProcAddr(device, "vkGetBufferMemoryRequirements2KHR")) {
        delete value;
        return -3;
    }
    value->backend.vkDeviceProcAddr = Fsr3GetDeviceProcAddr;
    int code = static_cast<int>(createContext(&value->effect, &value->upscale.header, nullptr));
    if (code != 0) { delete value; return code; }
    *result = value;
    return 0;
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

/// @brief Records reconstruction using matching temporal constants and independent R8 reactive/composition masks.
/// @details Inputs must already have SDK-compatible compute-read usage and output storage-write usage. The call records GPU work without submitting or waiting; all borrowed images remain live through host timeline completion.
/// @param context Borrowed live SR owner.
/// @param frame Borrowed 184-byte frame record with render-resolution material/composition masks and matching input/output extents.
/// @return SDK dispatch result; -1 for absent context/frame/dispatch, -2 for incompatible required image formats.
extern "C" __declspec(dllexport) int VulkanStoryFsr3EvaluateWithComposition(
    VulkanStoryFsr3Context* context, const VulkanStoryFsr3Frame* frame) {
    if (!context || !frame || !dispatch) return -1;
    if (frame->color.format != VK_FORMAT_R16G16B16A16_SFLOAT ||
        frame->depth.format != VK_FORMAT_D32_SFLOAT ||
        frame->motion.format != VK_FORMAT_R16G16_SFLOAT ||
        frame->output.format != VK_FORMAT_R16G16B16A16_SFLOAT ||
        frame->reactive.format != VK_FORMAT_R8_UNORM ||
        frame->composition.format != VK_FORMAT_R8_UNORM) return -2;
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
    desc.frameTimeDelta = frame->deltaMs > 0 ? frame->deltaMs : 16.667f;
    desc.preExposure = 1.0f;
    desc.reset = frame->reset != 0;
    desc.cameraNear = frame->nearPlane;
    desc.cameraFar = frame->farPlane;
    desc.cameraFovAngleVertical = frame->fovRadians;
    desc.viewSpaceToMetersFactor = 1.0f;
    int code = static_cast<int>(dispatch(&context->effect, &desc.header));
    return code;
}

/// @brief Destroys the reconstruction effect then deletes its native owner on successful SDK release.
/// @details The caller must first complete Vulkan work referencing the effect. A nonzero SDK release result preserves the context pointer.
/// @return Zero for successful destruction/null, -1 when an existing effect has no destroy export, or the SDK release error.
extern "C" __declspec(dllexport) int VulkanStoryFsr3Destroy(VulkanStoryFsr3Context* context) {
    if (!context) return 0;
    if (context->effect && !destroyContext) return -1;
    int code = context->effect ? static_cast<int>(destroyContext(&context->effect, nullptr)) : 0;
    if (code != 0) return code;
    delete context;
    return 0;
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
extern "C" __declspec(dllexport) int VulkanStoryFsr3SwapchainCreate(
    VkPhysicalDevice physical, VkDevice device, VkQueue gameQueue, VkQueue asyncQueue,
    VkQueue presentQueue, VkQueue acquireQueue, uint32_t queueFamily,
    const VkSwapchainCreateInfoKHR* info, VulkanStoryFsr3Swapchain** result) {
    if (!createContext || !query || !destroyContext || !physical || !device ||
        !gameQueue || !asyncQueue || !presentQueue || !acquireQueue ||
        !info || !result || info->oldSwapchain) return -1;
    *result = nullptr;
    auto* value = new (std::nothrow) VulkanStoryFsr3Swapchain();
    if (!value) return -2;
    value->device = device;
    ffxCreateContextDescFrameGenerationSwapChainVK desc{};
    desc.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FGSWAPCHAIN_VK;
    desc.physicalDevice = physical;
    desc.device = device;
    desc.swapchain = &value->chain;
    desc.createInfo = *info;
    desc.createInfo.oldSwapchain = VK_NULL_HANDLE;
    // The FFX Vulkan proxy requires four distinct VkQueue handles even when
    // async frame-generation workloads are disabled. They can share one family.
    desc.gameQueue = {gameQueue, queueFamily, nullptr};
    desc.asyncComputeQueue = {asyncQueue, queueFamily, nullptr};
    desc.presentQueue = {presentQueue, queueFamily, nullptr};
    desc.imageAcquireQueue = {acquireQueue, queueFamily, nullptr};
    int code = static_cast<int>(createContext(&value->context, &desc.header, nullptr));
    if (code != 0) { delete value; return code; }
    value->functions.header.type = FFX_API_QUERY_DESC_TYPE_FGSWAPCHAIN_FUNCTIONS_VK;
    code = static_cast<int>(query(&value->context, &value->functions.header));
    if (code != 0 || !value->functions.pOutCreateSwapchainFFXAPI ||
        !value->functions.pOutDestroySwapchainFFXAPI ||
        !value->functions.pOutGetSwapchainImagesKHR ||
        !value->functions.pOutAcquireNextImageKHR ||
        !value->functions.pOutQueuePresentKHR) {
        destroyContext(&value->context, nullptr);
        delete value;
        return code != 0 ? code : -3;
    }
    *result = value;
    return 0;
}

/// @brief Returns the chain retained by the supplied SDK presentation owner.
/// @details The handle is borrowed and must be acquired/presented/destroyed through this owner.
/// @return Current chain, or VK_NULL_HANDLE for a null owner.
extern "C" __declspec(dllexport) VkSwapchainKHR VulkanStoryFsr3SwapchainHandle(
    VulkanStoryFsr3Swapchain* value) { return value ? value->chain : VK_NULL_HANDLE; }

/// @brief Creates a replacement chain through the existing SDK presentation context.
/// @details The owner's current-chain handle updates only on VK_SUCCESS.
/// @return Proxy Vulkan creation result, or VK_ERROR_INITIALIZATION_FAILED for missing required arguments.
extern "C" __declspec(dllexport) VkResult VulkanStoryFsr3SwapchainRecreate(
    VulkanStoryFsr3Swapchain* value, const VkSwapchainCreateInfoKHR* info,
    VkSwapchainKHR* chain) {
    if (!value || !info || !chain) return VK_ERROR_INITIALIZATION_FAILED;
    VkResult code = value->functions.pOutCreateSwapchainFFXAPI(
        value->device, info, nullptr, chain, value->context);
    if (code == VK_SUCCESS) value->chain = *chain;
    return code;
}

/// @brief Enumerates proxy-owned swapchain images with Vulkan count/query semantics.
/// @details Returned VkImages borrow chain lifetime and are consumed through the same presentation owner.
/// @return Proxy Vulkan result, or VK_ERROR_INITIALIZATION_FAILED for an absent owner/count pointer.
extern "C" __declspec(dllexport) VkResult VulkanStoryFsr3SwapchainGetImages(
    VulkanStoryFsr3Swapchain* value, VkSwapchainKHR chain, uint32_t* count, VkImage* images) {
    return value && count ? value->functions.pOutGetSwapchainImagesKHR(
        value->device, chain, count, images) : VK_ERROR_INITIALIZATION_FAILED;
}

/// @brief Acquires an image through the SDK replacement entry point.
/// @details Timeout, binary semaphore/fence and output index follow the borrowed Vulkan acquisition contract.
/// @return Proxy Vulkan result, or VK_ERROR_INITIALIZATION_FAILED for an absent owner/index pointer.
extern "C" __declspec(dllexport) VkResult VulkanStoryFsr3SwapchainAcquire(
    VulkanStoryFsr3Swapchain* value, VkSwapchainKHR chain, uint64_t timeout,
    VkSemaphore semaphore, VkFence fence, uint32_t* imageIndex) {
    return value && imageIndex ? value->functions.pOutAcquireNextImageKHR(
        value->device, chain, timeout, semaphore, fence, imageIndex) : VK_ERROR_INITIALIZATION_FAILED;
}

/// @brief Queues presentation through the SDK replacement dispatch.
/// @details Success does not establish GPU completion or scanout completion; presentation retirement uses the SDK drain operation.
/// @return Proxy Vulkan result, or VK_ERROR_INITIALIZATION_FAILED for an absent owner/present description.
extern "C" __declspec(dllexport) VkResult VulkanStoryFsr3SwapchainPresent(
    VulkanStoryFsr3Swapchain* value, VkQueue queue, const VkPresentInfoKHR* info) {
    return value && info ? value->functions.pOutQueuePresentKHR(queue, info) :
        VK_ERROR_INITIALIZATION_FAILED;
}

/// @brief Disables linked interpolation for this chain and waits for SDK presentation work.
/// @details A failed disable/drain prevents the caller from destroying the chain or retiring tagged inputs.
/// @return Zero for a null chain or successful preparation, -1 for missing prerequisites, otherwise the SDK failure.
extern "C" __declspec(dllexport) int VulkanStoryFsr3SwapchainPrepareDestroy(
    VulkanStoryFsr3Swapchain* value, VkSwapchainKHR chain) {
    if (!chain) return 0;
    if (!value || !value->context || !dispatch) return -1;
    if (value->activeFg && value->activeFg->chain == chain) {
        int disabled = VulkanStoryFsr3FgDisable(value->activeFg, value);
        if (disabled != 0) return disabled;
    }
    ffxDispatchDescFrameGenerationSwapChainWaitForPresentsVK wait{};
    wait.header.type = FFX_API_DISPATCH_DESC_TYPE_FGSWAPCHAIN_WAIT_FOR_PRESENTS_VK;
    return static_cast<int>(dispatch(&value->context, &wait.header));
}

/// @brief Prepares release and destroys the specified chain through its SDK owner.
/// @details Checked preparation failure preserves the chain/ownership association. Successful release clears the current-chain field only when it matches.
/// @return Zero on success/null chain, -1 for missing destruction dispatch, or the preparation error.
extern "C" __declspec(dllexport) int VulkanStoryFsr3SwapchainDestroyChainChecked(
    VulkanStoryFsr3Swapchain* value, VkSwapchainKHR chain) {
    int prepared = VulkanStoryFsr3SwapchainPrepareDestroy(value, chain);
    if (prepared != 0) return prepared;
    if (!chain) return 0;
    if (!value->functions.pOutDestroySwapchainFFXAPI) return -1;
    value->functions.pOutDestroySwapchainFFXAPI(value->device, chain, nullptr, value->context);
    if (value->chain == chain) value->chain = VK_NULL_HANDLE;
    return 0;
}

// Preserve the old export's signature. New managed callers require the checked
// export; legacy callers at least leave failed SDK owners intact.
/// @brief Legacy void chain destruction that logs failed checked preparation while retaining ownership.
/// @details New callers use DestroyChainChecked to observe the release result.
extern "C" __declspec(dllexport) void VulkanStoryFsr3SwapchainDestroyChain(
    VulkanStoryFsr3Swapchain* value, VkSwapchainKHR chain) {
    int code = VulkanStoryFsr3SwapchainDestroyChainChecked(value, chain);
    if (code != 0)
        std::fprintf(stderr, "[FidelityFX] swapchain destruction retained ownership after error %d\n", code);
}

/// @brief Queries the SDK's actual presentation count for the supplied proxy chain.
/// @return SDK count, or zero for missing owner/chain/count function.
extern "C" __declspec(dllexport) uint64_t VulkanStoryFsr3SwapchainLastPresentCount(
    VulkanStoryFsr3Swapchain* value, VkSwapchainKHR chain) {
    return value && chain && value->functions.pOutGetLastPresentCountFFXAPI ?
        value->functions.pOutGetLastPresentCountFFXAPI(chain) : 0;
}

/// @brief Disables linked interpolation, drains presentation and releases the SDK presentation context.
/// @details Failure before SDK destruction completion leaves the native owner attached. The host retains the Vulkan device/queues until this owner is released.
/// @return Zero on successful destruction/null, -1 for a missing required export, or disable/drain/destruction error.
extern "C" __declspec(dllexport) int VulkanStoryFsr3SwapchainDestroy(
    VulkanStoryFsr3Swapchain* value) {
    if (!value) return 0;
    if (value->activeFg) {
        int disabled = VulkanStoryFsr3FgDisable(value->activeFg, value);
        if (disabled != 0) return disabled;
    }
    int prepared = VulkanStoryFsr3SwapchainPrepareDestroy(value, value->chain);
    if (prepared != 0) return prepared;
    if (value->context && !destroyContext) return -1;
    int code = value->context ? static_cast<int>(destroyContext(&value->context, nullptr)) : 0;
    if (code != 0) return code;
    delete value;
    return 0;
}

/// @brief Creates a Vulkan interpolation effect with explicit display extent and HUD-free RGBA format metadata.
/// @details The selected backbuffer format can be BGRA while the renderer's upright HUD-free source remains RGBA. Device/physical-device handles are borrowed.
/// @return Zero on success, -1 for prerequisites, -2 for allocation failure, -3 for Vulkan dispatch absence, or the SDK creation result.
extern "C" __declspec(dllexport) int VulkanStoryFsr3FgCreate(
    VkDevice device, VkPhysicalDevice physical, uint32_t width, uint32_t height, VkFormat backBufferFormat,
    VulkanStoryFsr3FgContext** result) {
    if (!createContext || !result || !device || !physical || !width || !height) return -1;
    auto* value = new (std::nothrow) VulkanStoryFsr3FgContext();
    if (!value) return -2;
    value->width = width;
    value->height = height;
    value->frameGeneration.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION;
    value->frameGeneration.header.pNext = &value->hudlessFormat.header;
    value->frameGeneration.displaySize = {width, height};
    value->frameGeneration.maxRenderSize = {width, height};
    value->frameGeneration.backBufferFormat = backBufferFormat == VK_FORMAT_B8G8R8A8_UNORM ?
        FFX_API_SURFACE_FORMAT_B8G8R8A8_UNORM : FFX_API_SURFACE_FORMAT_R8G8B8A8_UNORM;
    // The proxy's real backbuffer may be BGRA, while the Vulkan renderer's
    // upright HUD-less image is RGBA. FFX keeps a previous HUD-less source in
    // this format; without the linked descriptor it assumes backBufferFormat.
    value->hudlessFormat.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION_HUDLESS;
    value->hudlessFormat.header.pNext = &value->backend.header;
    value->hudlessFormat.hudlessBackBufferFormat = FFX_API_SURFACE_FORMAT_R8G8B8A8_UNORM;
    value->backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK;
    value->backend.vkDevice = device;
    value->backend.vkPhysicalDevice = physical;
    HMODULE vulkan = GetModuleHandleW(L"vulkan-1.dll");
    vulkanGetDeviceProcAddr = vulkan ? reinterpret_cast<PFN_vkGetDeviceProcAddr>(
        GetProcAddress(vulkan, "vkGetDeviceProcAddr")) : nullptr;
    if (!vulkanGetDeviceProcAddr) { delete value; return -3; }
    value->backend.vkDeviceProcAddr = Fsr3GetDeviceProcAddr;
    int code = static_cast<int>(createContext(&value->effect, &value->frameGeneration.header, nullptr));
    if (code != 0) { delete value; return code; }
    *result = value;
    return 0;
}

/// @brief SDK presentation callback that dispatches interpolation through its borrowed effect owner.
/// @details The context must remain live until proxy presentation has drained.
/// @return FidelityFX dispatch result, or its parameter error for a missing context/description/function.
static ffxReturnCode_t Fsr3GenerateOnPresent(ffxDispatchDescFrameGeneration* params, void* user) {
    auto* context = static_cast<VulkanStoryFsr3FgContext*>(user);
    return context && params && dispatch ?
        dispatch(&context->effect, &params->header) : FFX_API_RETURN_ERROR_PARAMETER;
}

/// @brief Disables proxy interpolation, drains matching SDK presents and clears registered UI ownership.
/// @details The stored swapchain association is authoritative. Associations are cleared only after every SDK operation succeeds; on failure keep the effect and tagged inputs live.
/// @return Zero for an unassociated/null context or success, -1 for missing prerequisites, otherwise the SDK operation error.
extern "C" __declspec(dllexport) int VulkanStoryFsr3FgDisable(
    VulkanStoryFsr3FgContext* context, VulkanStoryFsr3Swapchain* swapchain) {
    if (!context || !context->chain) return 0;
    if (!context->effect || !configure) return -1;
    if (swapchain != context->swapchain) swapchain = context->swapchain;
    if (!swapchain || !swapchain->context || swapchain->chain != context->chain) return -1;
    ffxConfigureDescFrameGeneration config{};
    config.header.type = FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION;
    config.swapChain = reinterpret_cast<void*>(context->chain);
    config.frameGenerationEnabled = false;
    int code = static_cast<int>(configure(&context->effect, &config.header));
    if (code != 0) return code;
    if (swapchain && swapchain->context && swapchain->chain == context->chain) {
        ffxDispatchDescFrameGenerationSwapChainWaitForPresentsVK wait{};
        wait.header.type = FFX_API_DISPATCH_DESC_TYPE_FGSWAPCHAIN_WAIT_FOR_PRESENTS_VK;
        int waited = dispatch ? static_cast<int>(dispatch(&swapchain->context, &wait.header)) : -1;
        if (waited != 0) return waited;
        ffxConfigureDescFrameGenerationSwapChainRegisterUiResourceVK ui{};
        ui.header.type = FFX_API_CONFIGURE_DESC_TYPE_FGSWAPCHAIN_REGISTERUIRESOURCE_VK;
        int cleared = static_cast<int>(configure(&swapchain->context, &ui.header));
        if (cleared != 0) return cleared;
    }
    // Keep the ownership links when disable/drain failed. The caller must not
    // retire tagged inputs or destroy the effect without successful drain.
    if (swapchain && swapchain->activeFg == context) swapchain->activeFg = nullptr;
    context->swapchain = nullptr;
    context->chain = VK_NULL_HANDLE;
    return code;
}

/// @brief Configures interpolation and records matching depth/motion/camera preparation for the selected presentation path.
/// @details Direct mode records one generated output on the host command buffer. Proxy mode links the effect callback and registers premultiplied UI with internal SDK double buffering. Motion arrives in upright space and its vertical scale is negated here.
/// @param context Borrowed interpolation owner.
/// @param frame Borrowed matching frame resources/constants; null swapchainContext selects direct mode.
/// @return SDK configuration/dispatch result; -1 for missing prerequisites or -2 for unsupported resources/path metadata.
extern "C" __declspec(dllexport) int VulkanStoryFsr3FgEvaluate(
    VulkanStoryFsr3FgContext* context, const VulkanStoryFsr3FgFrame* frame) {
    if (!context || !frame || !dispatch || !configure) return -1;
    auto* swapchain = static_cast<VulkanStoryFsr3Swapchain*>(frame->swapchainContext);
    const bool directInterpolation = frame->swapchainContext == nullptr;
    if (frame->color.format != VK_FORMAT_R8G8B8A8_UNORM ||
        frame->ui.format != VK_FORMAT_R8G8B8A8_UNORM ||
        frame->depth.format != VK_FORMAT_D32_SFLOAT ||
        frame->motion.format != VK_FORMAT_R16G16_SFLOAT ||
        !frame->commands ||
        (directInterpolation ? (!frame->output.image ||
            frame->output.format != VK_FORMAT_R8G8B8A8_UNORM ||
            frame->output.width != context->width || frame->output.height != context->height) :
            (!swapchain->context || !swapchain->chain))) return -2;

    if (context->swapchain && context->swapchain != swapchain) {
        int disabled = VulkanStoryFsr3FgDisable(context, context->swapchain);
        if (disabled != 0) return disabled;
    }

    ffxConfigureDescFrameGeneration config{};
    config.header.type = FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION;
    // The interpolation effect is independent of the SDK's multi-queue
    // swapchain proxy. A host-owned presentation path can dispatch it on its
    // existing command buffer without fabricating additional VkQueue handles.
    if (directInterpolation) {
        config.flags = FFX_FRAMEGENERATION_FLAG_NO_SWAPCHAIN_CONTEXT_NOTIFY;
    } else {
        config.swapChain = reinterpret_cast<void*>(swapchain->chain);
        config.frameGenerationCallback = Fsr3GenerateOnPresent;
        config.frameGenerationCallbackUserContext = context;
    }
    config.frameGenerationEnabled = true;
    config.HUDLessColor = frame->hudless.image ? Resource(frame->hudless) : FfxApiResource{};
    config.generationRect = {0, 0, static_cast<int32_t>(context->width), static_cast<int32_t>(context->height)};
    config.frameID = frame->frameId;
    int code = static_cast<int>(configure(&context->effect, &config.header));
    if (code != 0) return code;
    if (!directInterpolation) {
        context->chain = swapchain->chain;
        context->swapchain = swapchain;
        swapchain->activeFg = context;
    }

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
    prepare.frameTimeDelta = frame->deltaMs > 0 ? frame->deltaMs : 16.667f;
    prepare.cameraNear = frame->nearPlane;
    prepare.cameraFar = frame->farPlane;
    prepare.cameraFovAngleVertical = frame->fovRadians;
    prepare.viewSpaceToMetersFactor = 1.0f;
    prepare.depth = Resource(frame->depth);
    prepare.motionVectors = Resource(frame->motion);
    code = static_cast<int>(dispatch(&context->effect, &prepare.header));
    if (code != 0) return code;

    if (directInterpolation) {
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
        generate.generationRect = config.generationRect;
        generate.frameID = frame->frameId;
        return static_cast<int>(dispatch(&context->effect, &generate.header));
    }

    ffxConfigureDescFrameGenerationSwapChainRegisterUiResourceVK ui{};
    ui.header.type = FFX_API_CONFIGURE_DESC_TYPE_FGSWAPCHAIN_REGISTERUIRESOURCE_VK;
    ui.uiResource = Resource(frame->ui);
    ui.flags = FFX_FRAMEGENERATION_UI_COMPOSITION_FLAG_USE_PREMUL_ALPHA |
        FFX_FRAMEGENERATION_UI_COMPOSITION_FLAG_ENABLE_INTERNAL_UI_DOUBLE_BUFFERING;
    return static_cast<int>(configure(&swapchain->context, &ui.header));
}

// Explicit ABI capability prevents pairing a direct-present host with an older bridge.
/// @brief Requires the direct-interpolation ABI and delegates a frame with no swapchain owner.
/// @details This path records exactly one generated output for the host's own presentation sequence.
/// @return The shared evaluation result, or -1 for a null frame/nonnull swapchainContext.
extern "C" __declspec(dllexport) int VulkanStoryFsr3FgEvaluateDirect(
    VulkanStoryFsr3FgContext* context, const VulkanStoryFsr3FgFrame* frame) {
    if (!frame || frame->swapchainContext) return -1;
    return VulkanStoryFsr3FgEvaluate(context, frame);
}

/// @brief Disables/drains any linked proxy effect and destroys the interpolation context.
/// @details The host must separately complete Vulkan work recorded by direct interpolation before calling. A nonzero disable or SDK destruction result retains this owner.
/// @return Zero on successful destruction/null, otherwise the disable/destruction result.
extern "C" __declspec(dllexport) int VulkanStoryFsr3FgDestroy(VulkanStoryFsr3FgContext* context) {
    if (!context) return 0;
    int disabled = VulkanStoryFsr3FgDisable(context, context->swapchain);
    if (disabled != 0) return disabled;
    int code = context->effect && destroyContext
        ? static_cast<int>(destroyContext(&context->effect, nullptr)) : 0;
    if (code != 0) return code;
    delete context;
    return 0;
}
