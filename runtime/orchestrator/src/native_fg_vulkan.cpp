// SPDX-License-Identifier: MIT
// Native FidelityFX 3 Vulkan leaf. The owner state lives in vulkan_ffx_fg.rs.
#include "native_fg_ffx_vulkan.h"
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include "vulkan_timing_abi.h"

#if defined(RSF_HAVE_FFX_VK)
#include <windows.h>
#include <ffx_api/ffx_framegeneration.h>
#include <ffx_api/vk/ffx_api_vk.h>
#include "vulkan_fg_copy.h"
#include "vulkan_fg_resources_native.h"
#include "vulkan_fg_wsi.h"
#include "../../backends/common/frame_capture_vk.h"
#include "../../vulkan/fsr3/wsi-dispatch.h"
#include <cstring>
#include <new>

// The Rust owner relies on these Vulkan values and on the ops layout.
static_assert(VK_FORMAT_R8G8B8A8_UNORM == 37 && VK_FORMAT_B8G8R8A8_UNORM == 44 && VK_FORMAT_R16G16_SFLOAT == 83 &&
    VK_FORMAT_R32_SFLOAT == 100 && VK_COLOR_SPACE_SRGB_NONLINEAR_KHR == 0);
static_assert(VK_SUCCESS == 0 && VK_INCOMPLETE == 5 && VK_SUBOPTIMAL_KHR == 1000001003 && VK_ERROR_INITIALIZATION_FAILED == -3);
static_assert(FFX_API_RETURN_OK == 0);
static_assert(sizeof(rsf_ffxgvk_swapchain) == 32 && sizeof(rsf_ffxgvk_ops) == 27 * sizeof(void*));
static_assert(sizeof(rsf_ffxgvk_open_ops) == 10 * sizeof(void*) &&
    sizeof(rsf_ffxgvk_core_entries) == 8 * sizeof(void*) && sizeof(rsf_ffxgvk_wsi_entries) == 6 * sizeof(void*));

extern "C" const char* rsf_core_vk_promoted_device_proc_alias(const char* name);

struct rsf_ffxgvk_native {
    rsf_fgvk_setup setup{};
    rsf_vkffx_exports sdk;
    ffxContext swapchain = nullptr, effect = nullptr;
    VkSwapchainKHR chain = VK_NULL_HANDLE;
    PfnFfxConfigure configure = nullptr;
    rsf_ffx_set_wsi_dispatch wsi_dispatch = nullptr;
    PFN_vkQueuePresentKHR native_present = nullptr;
    PFN_vkAcquireNextImageKHR native_acquire = nullptr;
    PFN_vkGetSwapchainImagesKHR native_images = nullptr;
    PFN_vkCreateSwapchainKHR native_create = nullptr;
    PFN_vkDestroySwapchainKHR native_destroy = nullptr;
    ffxQueryDescSwapchainReplacementFunctionsVK functions{};
    rsf::vkfg::Device gpu;
    PFN_vkCmdPipelineBarrier barrier = nullptr; PFN_vkCmdCopyImage copy = nullptr;
    PFN_vkDeviceWaitIdle idle = nullptr;
    VkPhysicalDeviceMemoryProperties memory_properties{};
    VkAllocationCallbacks* allocator = nullptr;
    uint32_t width = 0, height = 0;
    // Each readback must identify a recording that executed, even after a
    // command buffer address is reused. The owner flushes after a full drain.
    rsf::FrameCaptureVk capture{rsf::FrameCaptureVk::Completion::command_retirement};
};
namespace {
// The WSI hooks carry no user pointer. Each borrows the leaf from the Rust
// route for the whole callback, including the forwarded Vulkan call; the
// route keeps the leaf alive meanwhile and holds no lock.
struct Borrow {
    rsf_ffxgvk_native* self;
    explicit Borrow(const void* device, uint32_t match_device = 1) : self(rsf_core_ffxgvk_wsi_borrow(device, match_device)) {}
    ~Borrow() { if (self) rsf_core_ffxgvk_wsi_return(); }
    Borrow(const Borrow&) = delete;
    Borrow& operator=(const Borrow&) = delete;
};
VkResult VKAPI_PTR serialized_create(VkDevice device, const VkSwapchainCreateInfoKHR* info, const VkAllocationCallbacks* allocator, VkSwapchainKHR* chain) {
    const Borrow borrow(device);
    const auto call = borrow.self ? borrow.self->native_create : nullptr;
    return call ? rsf_core_vkfg_wsi_create(rsf_slvk_without_dlssg, call, device, info, allocator, chain)
        : VK_ERROR_INITIALIZATION_FAILED;
}
void VKAPI_PTR serialized_destroy(VkDevice device, VkSwapchainKHR chain, const VkAllocationCallbacks* allocator) {
    const Borrow borrow(device);
    const auto call = borrow.self ? borrow.self->native_destroy : nullptr;
    if (call) rsf_core_vkfg_wsi_destroy(rsf_slvk_without_dlssg, call, device, chain, allocator);
}
VkResult VKAPI_PTR serialized_acquire(VkDevice device, VkSwapchainKHR chain, uint64_t timeout, VkSemaphore semaphore, VkFence fence, uint32_t* index) {
    const auto start = rsf_core_vk_timing_begin();
    const auto result = [&]() {
        const Borrow borrow(device);
        const auto call = borrow.self ? borrow.self->native_acquire : nullptr;
        return call ? rsf_core_vkfg_wsi_acquire(rsf_slvk_without_dlssg, rsf_latencyvk_wsi_call, call, device, chain, timeout, semaphore, fence, index)
            : VK_ERROR_INITIALIZATION_FAILED;
    }();
    rsf_core_vk_timing_end("native_acquire_total", 0, start);
    return result;
}
VkResult VKAPI_PTR serialized_images(VkDevice device, VkSwapchainKHR chain, uint32_t* count, VkImage* images) {
    const Borrow borrow(device);
    const auto call = borrow.self ? borrow.self->native_images : nullptr;
    return call ? rsf_core_vkfg_wsi_images(rsf_slvk_without_dlssg, rsf_latencyvk_wsi_call, call, device, chain, count, images)
        : VK_ERROR_INITIALIZATION_FAILED;
}
VkResult VKAPI_PTR serialized_present(VkQueue queue, const VkPresentInfoKHR* info) {
    const auto start = rsf_core_vk_timing_begin();
    const auto result = [&]() {
        const Borrow borrow(nullptr, 0);
        const auto call = borrow.self ? borrow.self->native_present : nullptr;
        return call ? rsf_core_vkfg_wsi_present(rsf_slvk_without_dlssg, rsf_latencyvk_queue_present, call, queue, info)
            : VK_ERROR_INITIALIZATION_FAILED;
    }();
    rsf_core_vk_timing_end("native_present_total", 0, start);
    return result;
}
// Keep both host loader calls in this native leaf. Rust selects the alias after a miss.
PFN_vkVoidFunction device_proc_for(const rsf_ffxgvk_native& self, VkDevice device, const char* name) {
    auto proc = self.setup.graphics.device_proc;
    auto value = proc(device, name);
    if (!value) {
        if (const auto* alias = rsf_core_vk_promoted_device_proc_alias(name)) value = proc(device, alias);
    }
    return reinterpret_cast<PFN_vkVoidFunction>(value);
}
// SDK backend vkDeviceProcAddr, also without a user pointer.
PFN_vkVoidFunction VKAPI_PTR device_proc(VkDevice device, const char* name) {
    const Borrow borrow(device);
    return borrow.self ? device_proc_for(*borrow.self, device, name) : nullptr;
}
// SDK game-queue submit function, also without a user pointer.
VkResult graphics_submit(uint32_t count, const VkSubmitInfo* submits, VkFence fence) {
    const Borrow borrow(nullptr, 0);
    auto* self = borrow.self;
    return self ? VkResult(self->setup.graphics_submit(self->setup.queue_user, count, submits, reinterpret_cast<uint64_t>(fence))) : VK_ERROR_DEVICE_LOST;
}
struct QueueLock {
    rsf_ffxgvk_native& self;
    explicit QueueLock(rsf_ffxgvk_native& value) : self(value) { self.setup.lock_queues(self.setup.queue_user); }
    ~QueueLock() { self.setup.unlock_queues(self.setup.queue_user); }
};
template<class T> T handle(uint64_t value) { return reinterpret_cast<T>(static_cast<uintptr_t>(value)); }
template<class T> uint64_t handle_value(T value) { return static_cast<uint64_t>(reinterpret_cast<uintptr_t>(value)); }
FfxApiResource resource(const rsf_srvk_image& image) {
    FfxApiResource value{}; value.resource = handle<void*>(image.image);
    value.description.type = FFX_API_RESOURCE_TYPE_TEXTURE2D;
    value.description.width = image.width; value.description.height = image.height; value.description.depth = value.description.mipCount = 1;
    value.description.format = ffxApiGetSurfaceFormatVK(VkFormat(image.format));
    value.description.usage = FFX_API_RESOURCE_USAGE_READ_ONLY; value.state = FFX_API_RESOURCE_STATE_COMPUTE_READ;
    return value;
}
void record_capture(rsf_ffxgvk_native& self, VkCommandBuffer commands, const rsf_srvk_image& image, const rsf_frame_record& record,
    const char* role, uint32_t reset) noexcept try {
    (void)self.capture.record(commands, image, record, role, reset);
} catch (...) {}
ffxReturnCode_t generate(ffxDispatchDescFrameGeneration* data, void* userdata) {
    auto* native = static_cast<rsf_ffxgvk_native*>(userdata);
    rsf_frame_record record{};
    if (!data || !rsf_core_ffxgvk_generation_record(native, data->frameID, &record)) return FFX_API_RETURN_ERROR_PARAMETER;
    auto& self = *native;
    data->reset |= (record.flags & RSF_FRAME_FLAG_RESET) != 0;
    auto result = self.sdk.dispatch(&self.effect, &data->header);
    if (result == FFX_API_RETURN_OK) {
        rsf_core_ffxgvk_note_generated(native);
        if (self.capture.wants(data->frameID)) for (uint32_t i = 0; i < data->numGeneratedFrames && i < 4; ++i) {
            const auto& source = data->outputs[i];
            rsf_srvk_image image{self.setup.graphics.device, handle_value(source.resource),
                uint32_t(VK_FORMAT_R8G8B8A8_UNORM), source.description.width, source.description.height, VK_IMAGE_LAYOUT_GENERAL, 1};
            // The generated output uses the actual swapchain channel order.
            if (source.description.format == FFX_API_SURFACE_FORMAT_B8G8R8A8_UNORM) image.format = VK_FORMAT_B8G8R8A8_UNORM;
            (void)self.capture.record(static_cast<VkCommandBuffer>(data->commandList), image, record, "fg-generated-before-ui", data->reset, false);
        }
    }
    return result;
}

rsf_ffxgvk_native* allocate_native(const rsf_fgvk_setup* setup, uint32_t width, uint32_t height) noexcept try {
    auto* self = new (std::nothrow) rsf_ffxgvk_native{};
    if (!self) return nullptr;
    self->setup = *setup;
    self->width = width;
    self->height = height;
    self->allocator = const_cast<VkAllocationCallbacks*>(static_cast<const VkAllocationCallbacks*>(setup->allocator));
    return self;
} catch (...) { return nullptr; }

void close_native(rsf_ffxgvk_native* self) noexcept { delete self; }

int32_t lookup_device(rsf_ffxgvk_native* self, const char* name, uint32_t aliases,
    rsf_vk_function* out) noexcept try {
    const auto& graphics = self->setup.graphics;
    *out = aliases
        ? reinterpret_cast<rsf_vk_function>(device_proc_for(*self, static_cast<VkDevice>(graphics.device), name))
        : graphics.device_proc(graphics.device, name);
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t lookup_instance(rsf_ffxgvk_native* self, const char* name, rsf_vk_function* out) noexcept try {
    const auto& graphics = self->setup.graphics;
    *out = graphics.instance_proc(graphics.instance, name);
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

void bind_core(rsf_ffxgvk_native* self, const rsf_ffxgvk_core_entries* entries) noexcept {
    const auto& [configure, create, destroy, query, dispatch, barrier, copy, idle] = *entries;
    self->configure = reinterpret_cast<PfnFfxConfigure>(configure);
    self->sdk.create = reinterpret_cast<PfnFfxCreateContext>(create);
    self->sdk.destroy = reinterpret_cast<PfnFfxDestroyContext>(destroy);
    self->sdk.query = reinterpret_cast<PfnFfxQuery>(query);
    self->sdk.dispatch = reinterpret_cast<PfnFfxDispatch>(dispatch);
    self->barrier = reinterpret_cast<PFN_vkCmdPipelineBarrier>(barrier);
    self->copy = reinterpret_cast<PFN_vkCmdCopyImage>(copy);
    self->idle = reinterpret_cast<PFN_vkDeviceWaitIdle>(idle);
}

int32_t read_memory(rsf_ffxgvk_native* self, rsf_vk_function query) noexcept try {
    const auto memory = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(query);
    memory(static_cast<VkPhysicalDevice>(self->setup.graphics.physical_device), &self->memory_properties);
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t load_resources(rsf_ffxgvk_native* self) noexcept try {
    if (!self->gpu.load(self->setup, false)) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    self->gpu.memory_types(self->memory_properties);
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t initialize_capture(rsf_ffxgvk_native* self, void** resources) noexcept try {
    const auto& graphics = self->setup.graphics;
    if (!self->capture.initialize(static_cast<VkDevice>(graphics.device), graphics.device_proc, self->memory_properties))
        return RSF_BACKEND_ERROR_INIT_FAILED;
    *resources = self->gpu.get();
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

void bind_wsi(rsf_ffxgvk_native* self, const rsf_ffxgvk_wsi_entries* entries) noexcept {
    const auto& [dispatch, present, acquire, images, create, destroy] = *entries;
    self->wsi_dispatch = reinterpret_cast<rsf_ffx_set_wsi_dispatch>(dispatch);
    self->native_present = reinterpret_cast<PFN_vkQueuePresentKHR>(present);
    self->native_acquire = reinterpret_cast<PFN_vkAcquireNextImageKHR>(acquire);
    self->native_images = reinterpret_cast<PFN_vkGetSwapchainImagesKHR>(images);
    self->native_create = reinterpret_cast<PFN_vkCreateSwapchainKHR>(create);
    self->native_destroy = reinterpret_cast<PFN_vkDestroySwapchainKHR>(destroy);
}

constexpr rsf_ffxgvk_open_ops open_ops{
    allocate_native, close_native, rsf_vkffx_symbol, lookup_device, lookup_instance,
    bind_core, read_memory, load_resources, initialize_capture, bind_wsi,
};

int32_t open(const rsf_fgvk_setup* setup, void* module, uint32_t width, uint32_t height,
    rsf_ffxgvk_native** out, void** resources) noexcept {
    return rsf_core_ffxgvk_open(&open_ops, setup, module, width, height, out, resources);
}

int32_t set_wsi(rsf_ffxgvk_native* self, uint32_t enabled) noexcept {
    const rsf_ffx_wsi_dispatch dispatch{sizeof(dispatch), 2, static_cast<VkDevice>(self->setup.graphics.device),
        enabled ? serialized_create : nullptr, enabled ? serialized_destroy : nullptr,
        enabled ? serialized_present : nullptr, enabled ? serialized_acquire : nullptr,
        enabled ? serialized_images : nullptr};
    return self->wsi_dispatch(&dispatch);
}

int32_t create_swapchain(rsf_ffxgvk_native* self, uint64_t* chain, uint32_t* has_context) noexcept {
    const auto& info = *static_cast<const VkSwapchainCreateInfoKHR*>(self->setup.create_info);
    const auto& setup = self->setup;
    ffxCreateContextDescFrameGenerationSwapChainVK create{}; create.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FGSWAPCHAIN_VK;
    create.physicalDevice = static_cast<VkPhysicalDevice>(setup.graphics.physical_device); create.device = static_cast<VkDevice>(setup.graphics.device);
    create.swapchain = &self->chain; create.allocator = self->allocator; create.createInfo = info;
    create.gameQueue = {static_cast<VkQueue>(setup.queues[0]), setup.queue_family, graphics_submit};
    create.asyncComputeQueue = {static_cast<VkQueue>(setup.queues[1]), setup.queue_family, nullptr};
    create.presentQueue = {static_cast<VkQueue>(setup.queues[2]), setup.queue_family, nullptr};
    create.imageAcquireQueue = {static_cast<VkQueue>(setup.queues[3]), setup.queue_family, nullptr};
    const auto created = self->sdk.create(&self->swapchain, &create.header, nullptr);
    *chain = handle_value(self->chain);
    *has_context = self->swapchain ? 1u : 0u;
    return created == FFX_API_RETURN_OK ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
}
int32_t query_swapchain(rsf_ffxgvk_native* self) noexcept {
    self->functions.header.type = FFX_API_QUERY_DESC_TYPE_FGSWAPCHAIN_FUNCTIONS_VK;
    if (self->sdk.query(&self->swapchain, &self->functions.header) != FFX_API_RETURN_OK || !self->functions.pOutGetSwapchainImagesKHR ||
        !self->functions.pOutAcquireNextImageKHR || !self->functions.pOutQueuePresentKHR || !self->functions.pOutDestroySwapchainFFXAPI)
        return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    return RSF_BACKEND_OK;
}
int32_t create_effect(rsf_ffxgvk_native* self, uint32_t* has_context) noexcept {
    const auto& info = *static_cast<const VkSwapchainCreateInfoKHR*>(self->setup.create_info);
    const auto& graphics = self->setup.graphics;
    ffxCreateBackendVKDesc backend{}; backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK;
    backend.vkDevice = static_cast<VkDevice>(graphics.device); backend.vkPhysicalDevice = static_cast<VkPhysicalDevice>(graphics.physical_device);
    backend.vkDeviceProcAddr = device_proc;
    ffxCreateContextDescFrameGenerationHudless hudless{}; hudless.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION_HUDLESS;
    hudless.header.pNext = &backend.header; hudless.hudlessBackBufferFormat = FFX_API_SURFACE_FORMAT_R8G8B8A8_UNORM;
    ffxCreateContextDescFrameGeneration fg{}; fg.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION; fg.header.pNext = &hudless.header;
    fg.flags = (graphics.depth_inverted ? FFX_FRAMEGENERATION_ENABLE_DEPTH_INVERTED : 0u) |
        (graphics.depth_infinite ? FFX_FRAMEGENERATION_ENABLE_DEPTH_INFINITE : 0u) |
        (graphics.motion_jittered ? FFX_FRAMEGENERATION_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION : 0u);
    fg.displaySize = fg.maxRenderSize = {self->width, self->height}; fg.backBufferFormat = ffxApiGetSurfaceFormatVK(info.imageFormat);
    const auto created = self->sdk.create(&self->effect, &fg.header, nullptr);
    *has_context = self->effect ? 1u : 0u;
    return created == FFX_API_RETURN_OK ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
}
int32_t query_provider(rsf_ffxgvk_native* self) noexcept {
    ffxQueryGetProviderVersion provider{}; provider.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
    if (self->sdk.query(&self->effect, &provider.header) != FFX_API_RETURN_OK || !provider.versionId) return RSF_BACKEND_ERROR_INIT_FAILED;
    self->capture.set_provider(provider.versionId, 3, true);
    return RSF_BACKEND_OK;
}
int32_t destroy_effect(rsf_ffxgvk_native* self) noexcept {
    if (self->effect && self->sdk.destroy(&self->effect, nullptr) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self->effect = nullptr;
    return RSF_BACKEND_OK;
}
void destroy_chain(rsf_ffxgvk_native* self) noexcept {
    if (self->chain && self->functions.pOutDestroySwapchainFFXAPI)
        self->functions.pOutDestroySwapchainFFXAPI(static_cast<VkDevice>(self->setup.graphics.device), self->chain, self->allocator, self->swapchain);
    self->chain = VK_NULL_HANDLE;
}
int32_t destroy_swapchain(rsf_ffxgvk_native* self) noexcept {
    if (self->swapchain && self->sdk.destroy(&self->swapchain, nullptr) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self->swapchain = nullptr;
    return RSF_BACKEND_OK;
}
int32_t images(rsf_ffxgvk_native* self, uint32_t* count, uint64_t* output) noexcept {
    return self->functions.pOutGetSwapchainImagesKHR(static_cast<VkDevice>(self->setup.graphics.device), self->chain, count,
        reinterpret_cast<VkImage*>(output));
}
int32_t acquire(rsf_ffxgvk_native* self, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index, uint64_t frame) noexcept {
    const auto start = rsf_core_vk_timing_begin();
    const auto result = self->functions.pOutAcquireNextImageKHR(static_cast<VkDevice>(self->setup.graphics.device), self->chain, timeout,
        handle<VkSemaphore>(semaphore), handle<VkFence>(fence), index);
    rsf_core_vk_timing_end("ffx_acquire", frame, start);
    return result;
}
int32_t prepare(rsf_ffxgvk_native* self, void* commands, const rsf_fgvk_frame* frame) noexcept {
    const auto& record = *frame->record;
    ffxDispatchDescFrameGenerationPrepare data{}; data.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE;
    ffxDispatchDescFrameGenerationPrepareCameraInfo camera{}; camera.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_CAMERAINFO;
    const auto* world = record.camera.view_to_world;
    for (uint32_t i = 0; i < 3; ++i) { camera.cameraPosition[i] = world[12 + i]; camera.cameraRight[i] = world[i];
        camera.cameraUp[i] = world[4 + i]; camera.cameraForward[i] = -world[8 + i]; }
    data.header.pNext = &camera.header; data.frameID = record.frame_id; data.commandList = commands;
    data.renderSize = {record.render_width, record.render_height};
    data.jitterOffset = {record.camera.jitter_pixels[0], record.camera.jitter_pixels[1]};
    data.motionVectorScale = {frame->motion_scale_x, frame->motion_scale_y}; data.frameTimeDelta = record.frame_time_ms;
    data.cameraNear = record.camera.near_plane; data.cameraFar = record.camera.far_plane;
    data.cameraFovAngleVertical = record.camera.vertical_fov_radians; data.viewSpaceToMetersFactor = 1;
    data.depth = resource(frame->depth); data.motionVectors = resource(frame->motion);
    const auto start = rsf_core_vk_timing_begin();
    const auto result = int32_t(self->sdk.dispatch(&self->effect, &data.header));
    rsf_core_vk_timing_end("ffx_prepare_sdk", record.frame_id, start);
    return result;
}
void record_barrier(rsf_ffxgvk_native* self, void* commands, const rsf_vkfg_barrier* plan) noexcept {
    rsf_core_vkfg_record_barrier(self->barrier, static_cast<VkCommandBuffer>(commands), plan);
}
void record_copy(rsf_ffxgvk_native* self, void* commands, const rsf_vkfg_image_copy* plan) noexcept {
    rsf_core_vkfg_record_copy(self->copy, static_cast<VkCommandBuffer>(commands), plan);
}
int32_t configure(rsf_ffxgvk_native* self, uint32_t enabled, uint64_t color, uint64_t frame, const char* stage) noexcept {
    ffxConfigureDescFrameGeneration config{}; config.header.type = FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION;
    config.swapChain = self->chain;
    config.frameGenerationEnabled = enabled != 0;
    config.allowAsyncWorkloads = false;
    config.frameGenerationCallback = generate; config.frameGenerationCallbackUserContext = self;
    config.frameID = frame;
    config.generationRect = {0, 0, int32_t(self->width), int32_t(self->height)};
    if (config.frameGenerationEnabled) config.HUDLessColor = resource({self->setup.graphics.device, color, VK_FORMAT_R8G8B8A8_UNORM,
        self->width, self->height, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1});
    const auto start = stage ? rsf_core_vk_timing_begin() : 0;
    const auto result = self->configure(&self->effect, &config.header);
    rsf_core_vk_timing_end(stage, frame, start);
    return result == FFX_API_RETURN_OK ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
int32_t present(rsf_ffxgvk_native* self, const void* info, uint64_t frame, uint64_t* count) noexcept {
    const auto start = rsf_core_vk_timing_begin();
    const auto result = self->functions.pOutQueuePresentKHR(static_cast<VkQueue>(self->setup.queues[0]), static_cast<const VkPresentInfoKHR*>(info));
    rsf_core_vk_timing_end("ffx_queue_present", frame, start);
    if (self->functions.pOutGetLastPresentCountFFXAPI) *count = self->functions.pOutGetLastPresentCountFFXAPI(self->chain);
    return result;
}
int32_t drain(rsf_ffxgvk_native* self) noexcept {
    QueueLock lock(*self);
    ffxDispatchDescFrameGenerationSwapChainWaitForPresentsVK wait{}; wait.header.type = FFX_API_DISPATCH_DESC_TYPE_FGSWAPCHAIN_WAIT_FOR_PRESENTS_VK;
    if (self->sdk.dispatch(&self->swapchain, &wait.header) != FFX_API_RETURN_OK || self->idle(static_cast<VkDevice>(self->setup.graphics.device)) != VK_SUCCESS)
        return RSF_BACKEND_ERROR_NOT_READY;
    return RSF_BACKEND_OK;
}
void capture_flush(rsf_ffxgvk_native* self) noexcept try {
    (void)self->capture.flush();
} catch (...) {}
constexpr rsf_ffxgvk_ops operations{
    rsf_vk_sdk_system_loader, open, close_native, set_wsi,
    create_swapchain, query_swapchain, create_effect, query_provider, destroy_effect, destroy_chain, destroy_swapchain,
    images, acquire, prepare, record_barrier, record_copy, configure, present, drain,
    [](rsf_ffxgvk_native* self, uint64_t frame) -> uint32_t { return self->capture.wants(frame) ? 1u : 0u; },
    [](rsf_ffxgvk_native* self, void* commands, const rsf_srvk_image* image, const rsf_frame_record* record, const char* role) {
        record_capture(*self, static_cast<VkCommandBuffer>(commands), *image, *record, role, record->flags & RSF_FRAME_FLAG_RESET);
    },
    capture_flush,
    [](rsf_ffxgvk_native* self, uint64_t frame) {
        self->gpu.wait_timing(frame ? "ffx_slot_fence_wait" : nullptr, frame);
    },
    rsf_core_vk_timing_begin, rsf_core_vk_timing_end, &rsf::vkfg::platform,
};
}

extern "C" rsf_backend_result rsf_ffxgvk_create(const rsf_fgvk_setup* setup, rsf_ffxgvk** out, uint64_t* chain) {
    return rsf_core_ffxgvk_create_native(&operations, setup, out, chain);
}
extern "C" int32_t rsf_ffxgvk_images(rsf_ffxgvk* self, uint32_t* count, uint64_t* images) { return rsf_core_ffxgvk_images(self, count, images); }
extern "C" int32_t rsf_ffxgvk_acquire(rsf_ffxgvk* self, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
    return rsf_core_ffxgvk_acquire(self, timeout, semaphore, fence, index);
}
extern "C" rsf_backend_result rsf_ffxgvk_enable(rsf_ffxgvk* self, uint32_t enabled) { return rsf_core_ffxgvk_enable(self, enabled); }
extern "C" rsf_backend_result rsf_ffxgvk_prepare(rsf_ffxgvk* self, void* commands, const rsf_fgvk_frame* frame) {
    return rsf_core_ffxgvk_prepare(self, commands, frame);
}
extern "C" rsf_backend_result rsf_ffxgvk_hudless(rsf_ffxgvk* self, void* commands, const rsf_fgvk_frame* frame) {
    return rsf_core_ffxgvk_hudless(self, commands, frame);
}
extern "C" void rsf_ffxgvk_submitted(rsf_ffxgvk* self, void* const* commands, uint32_t count) { rsf_core_ffxgvk_submitted(self, commands, count); }
extern "C" int32_t rsf_ffxgvk_present(rsf_ffxgvk* self, const void* info) { return rsf_core_ffxgvk_present_native(self, info, nullptr, 0); }
extern "C" int32_t rsf_ffxgvk_present_source(rsf_ffxgvk* self, const void* info, const rsf_frame_record* source) {
    return rsf_core_ffxgvk_present_native(self, info, source, 1);
}
extern "C" rsf_backend_result rsf_ffxgvk_get_status(rsf_ffxgvk* self, rsf_fgvk_status* status) { return rsf_core_ffxgvk_status(self, status); }
extern "C" rsf_backend_result rsf_ffxgvk_drain(rsf_ffxgvk* self) { return rsf_core_ffxgvk_drain(self); }
extern "C" rsf_backend_result rsf_ffxgvk_destroy(rsf_ffxgvk* self) { return rsf_core_ffxgvk_destroy(self); }
#else
// Disabled build: the Rust owner validates creation arguments and reports
// RSF_BACKEND_ERROR_NOT_COMPILED, so no owner can reach the other entries.
extern "C" rsf_backend_result rsf_ffxgvk_create(const rsf_fgvk_setup* setup, rsf_ffxgvk** out, uint64_t* chain) {
    return rsf_core_ffxgvk_create(nullptr, setup, nullptr, out, chain);
}
extern "C" int32_t rsf_ffxgvk_images(rsf_ffxgvk*, uint32_t*, uint64_t*) { return -3; }
extern "C" int32_t rsf_ffxgvk_acquire(rsf_ffxgvk*, uint64_t, uint64_t, uint64_t, uint32_t*) { return -3; }
extern "C" rsf_backend_result rsf_ffxgvk_enable(rsf_ffxgvk* self, uint32_t enabled) {
    return !self || enabled > 1 ? RSF_BACKEND_ERROR_INVALID_ARGUMENT : RSF_BACKEND_ERROR_NOT_COMPILED;
}
static rsf_backend_result disabled_frame(rsf_ffxgvk* self, void* commands, const rsf_fgvk_frame* frame) {
    if (!self || !commands || !frame || frame->struct_size < sizeof(*frame)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return frame->abi_version != RSF_FGVK_ABI_VERSION ? RSF_BACKEND_ERROR_ABI_MISMATCH : RSF_BACKEND_ERROR_NOT_COMPILED;
}
extern "C" rsf_backend_result rsf_ffxgvk_prepare(rsf_ffxgvk* self, void* commands, const rsf_fgvk_frame* frame) {
    return disabled_frame(self, commands, frame);
}
extern "C" rsf_backend_result rsf_ffxgvk_hudless(rsf_ffxgvk* self, void* commands, const rsf_fgvk_frame* frame) {
    return disabled_frame(self, commands, frame);
}
extern "C" void rsf_ffxgvk_submitted(rsf_ffxgvk*, void* const*, uint32_t) {}
extern "C" int32_t rsf_ffxgvk_present(rsf_ffxgvk*, const void*) { return -3; }
extern "C" int32_t rsf_ffxgvk_present_source(rsf_ffxgvk*, const void*, const rsf_frame_record*) { return -3; }
extern "C" rsf_backend_result rsf_ffxgvk_get_status(rsf_ffxgvk* self, rsf_fgvk_status* status) {
    if (!self || !status || status->struct_size < sizeof(*status)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return status->abi_version != RSF_FGVK_ABI_VERSION ? RSF_BACKEND_ERROR_ABI_MISMATCH : RSF_BACKEND_ERROR_NOT_COMPILED;
}
extern "C" rsf_backend_result rsf_ffxgvk_drain(rsf_ffxgvk* self) { return self ? RSF_BACKEND_ERROR_NOT_COMPILED : RSF_BACKEND_OK; }
extern "C" rsf_backend_result rsf_ffxgvk_destroy(rsf_ffxgvk*) { return RSF_BACKEND_OK; }
#endif
