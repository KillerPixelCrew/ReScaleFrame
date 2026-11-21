// SPDX-License-Identifier: MIT
// Native FidelityFX 3 Vulkan leaf. The owner state lives in vulkan_ffx_fg.rs.
#include "native_fg_ffx_vulkan.h"
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include "native_vulkan_timing.h"

#if defined(RSF_HAVE_FFX_VK)
#include <windows.h>
#include <ffx_api/ffx_framegeneration.h>
#include <ffx_api/vk/ffx_api_vk.h>
#include "vulkan_fg_resources_native.h"
#include "../../backends/common/frame_capture_vk.h"
#include "../../vulkan/fsr3/wsi-dispatch.h"
#include <cstring>
#include <filesystem>
#include <memory>

// The Rust owner relies on these Vulkan values and on the ops layout.
static_assert(VK_FORMAT_R8G8B8A8_UNORM == 37 && VK_FORMAT_B8G8R8A8_UNORM == 44 && VK_FORMAT_R16G16_SFLOAT == 83 &&
    VK_FORMAT_R32_SFLOAT == 100 && VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL == 5 && VK_COLOR_SPACE_SRGB_NONLINEAR_KHR == 0);
static_assert(VK_SUCCESS == 0 && VK_INCOMPLETE == 5 && VK_SUBOPTIMAL_KHR == 1000001003 && VK_ERROR_INITIALIZATION_FAILED == -3);
static_assert(FFX_API_RETURN_OK == 0);
static_assert(sizeof(rsf_ffxgvk_swapchain) == 32 && sizeof(rsf_ffxgvk_ops) == 23 * sizeof(void*));

struct rsf_ffxgvk_native {
    rsf_fgvk_setup setup{};
    HMODULE module = nullptr;
    ffxContext swapchain = nullptr, effect = nullptr;
    VkSwapchainKHR chain = VK_NULL_HANDLE;
    PfnFfxCreateContext create = nullptr;
    PfnFfxDestroyContext destroy = nullptr;
    PfnFfxQuery query = nullptr;
    PfnFfxDispatch dispatch = nullptr;
    PfnFfxConfigure configure = nullptr;
    rsf_ffx_set_wsi_dispatch wsi_dispatch = nullptr;
    bool wsi_registered = false;
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
    // The source Rust admitted at the last successful configure. The SDK calls
    // the generation callback synchronously inside queuePresent.
    rsf::FrameCaptureVk capture;
    // Rust destroys the effect, the swapchain and the slot resources first and
    // has closed its WSI route, so no hook borrows this storage. Rust holds
    // the process-wide admission until this returns. A refused unregister
    // (-3, another device's table) means these hooks are not registered.
    ~rsf_ffxgvk_native() {
        if (wsi_registered && wsi_dispatch) {
            const rsf_ffx_wsi_dispatch clear{sizeof(clear), 2, static_cast<VkDevice>(setup.graphics.device), nullptr, nullptr, nullptr, nullptr, nullptr};
            (void)wsi_dispatch(&clear);
        }
        if (module) FreeLibrary(module);
    }
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
    auto* self = borrow.self;
    if (!self || !self->native_create) return VK_ERROR_INITIALIZATION_FAILED;
    struct Lease { PFN_vkCreateSwapchainKHR call; VkDevice device; const VkSwapchainCreateInfoKHR* info; const VkAllocationCallbacks* allocator; VkSwapchainKHR* chain; };
    Lease lease{self->native_create, device, info, allocator, chain};
    // Create/destroy hooks take their own LL2 mutex. Suppress only DLSS-G;
    // never hold an outer WSI mutex across these notification hooks.
    return VkResult(rsf_slvk_without_dlssg([](void* user) -> int32_t {
        const auto& value = *static_cast<Lease*>(user); return value.call(value.device, value.info, value.allocator, value.chain);
    }, &lease));
}
void VKAPI_PTR serialized_destroy(VkDevice device, VkSwapchainKHR chain, const VkAllocationCallbacks* allocator) {
    const Borrow borrow(device);
    auto* self = borrow.self;
    if (!self || !self->native_destroy) return;
    struct Lease { PFN_vkDestroySwapchainKHR call; VkDevice device; VkSwapchainKHR chain; const VkAllocationCallbacks* allocator; };
    Lease lease{self->native_destroy, device, chain, allocator};
    (void)rsf_slvk_without_dlssg([](void* user) -> int32_t {
        const auto& value = *static_cast<Lease*>(user); value.call(value.device, value.chain, value.allocator); return VK_SUCCESS;
    }, &lease);
}
VkResult VKAPI_PTR serialized_acquire(VkDevice device, VkSwapchainKHR chain, uint64_t timeout, VkSemaphore semaphore, VkFence fence, uint32_t* index) {
    const rsf::VulkanTimingScope timing("native_acquire_total", 0);
    const Borrow borrow(device);
    auto* self = borrow.self;
    if (!self || !self->native_acquire) return VK_ERROR_INITIALIZATION_FAILED;
    struct Lease { PFN_vkAcquireNextImageKHR call; VkDevice device; VkSwapchainKHR chain; uint64_t timeout; VkSemaphore semaphore; VkFence fence; uint32_t* index; };
    Lease lease{self->native_acquire, device, chain, timeout, semaphore, fence, index};
    return VkResult(rsf_slvk_without_dlssg([](void* user) -> int32_t {
        return rsf_latencyvk_wsi_call([](void* data) -> int32_t {
            const auto& value = *static_cast<Lease*>(data);
            return rsf::vk_timed_call("native_acquire", 0, [&] {
                return value.call(value.device, value.chain, value.timeout, value.semaphore, value.fence, value.index);
            });
        }, user);
    }, &lease));
}
VkResult VKAPI_PTR serialized_images(VkDevice device, VkSwapchainKHR chain, uint32_t* count, VkImage* images) {
    const Borrow borrow(device);
    auto* self = borrow.self;
    if (!self || !self->native_images) return VK_ERROR_INITIALIZATION_FAILED;
    struct Lease { PFN_vkGetSwapchainImagesKHR call; VkDevice device; VkSwapchainKHR chain; uint32_t* count; VkImage* images; };
    Lease lease{self->native_images, device, chain, count, images};
    return VkResult(rsf_slvk_without_dlssg([](void* user) -> int32_t {
        return rsf_latencyvk_wsi_call([](void* data) -> int32_t {
            const auto& value = *static_cast<Lease*>(data); return value.call(value.device, value.chain, value.count, value.images);
        }, user);
    }, &lease));
}
VkResult VKAPI_PTR serialized_present(VkQueue queue, const VkPresentInfoKHR* info) {
    const rsf::VulkanTimingScope timing("native_present_total", 0);
    const Borrow borrow(nullptr, 0);
    auto* self = borrow.self;
    if (!self || !self->native_present) return VK_ERROR_INITIALIZATION_FAILED;
    struct Lease { PFN_vkQueuePresentKHR call; VkQueue queue; const VkPresentInfoKHR* info; } lease{self->native_present, queue, info};
    return VkResult(rsf_slvk_without_dlssg([](void* user) -> int32_t {
        const auto& value = *static_cast<Lease*>(user);
        return rsf::vk_timed_call("native_queue_present", 0, [&] {
            return rsf_latencyvk_queue_present(reinterpret_cast<rsf_vk_function>(value.call), value.queue, value.info);
        });
    }, &lease));
}
// Typed leaf: resolves through the host loader, falling back from the KHR
// names the SDK backend asks for to their core aliases.
PFN_vkVoidFunction device_proc_for(const rsf_ffxgvk_native& self, VkDevice device, const char* name) {
    auto proc = self.setup.graphics.device_proc;
    auto value = proc(device, name);
    if (!value) {
        struct Alias { const char* extension; const char* core; };
        constexpr Alias aliases[] = {{"vkGetBufferMemoryRequirements2KHR", "vkGetBufferMemoryRequirements2"},
            {"vkGetImageMemoryRequirements2KHR", "vkGetImageMemoryRequirements2"},
            {"vkBindBufferMemory2KHR", "vkBindBufferMemory2"}, {"vkBindImageMemory2KHR", "vkBindImageMemory2"}};
        for (const auto& alias : aliases) if (std::strcmp(name, alias.extension) == 0) value = proc(device, alias.core);
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
void transition(rsf_ffxgvk_native& self, VkCommandBuffer commands, VkImage image, VkImageLayout from, VkImageLayout to) {
    VkImageMemoryBarrier data{}; data.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
    data.oldLayout = from; data.newLayout = to;
    data.srcAccessMask = from == VK_IMAGE_LAYOUT_UNDEFINED ? 0 : VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    data.dstAccessMask = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    data.srcQueueFamilyIndex = data.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    data.image = image; data.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    self.barrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &data);
}
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
    auto result = self.dispatch(&self.effect, &data->header);
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

int32_t open(const rsf_fgvk_setup* setup, uint32_t width, uint32_t height, rsf_ffxgvk_native** out, void** resources) noexcept try {
    // Rust admitted this owner to the WSI route and publishes the leaf only
    // after open succeeds. Registration is the last fallible step, so a failed
    // open registers nothing; the destructor unloads what it loaded.
    auto self = std::make_unique<rsf_ffxgvk_native>(); self->setup = *setup;
    const auto& graphics = setup->graphics;
    self->width = width; self->height = height;
    self->allocator = const_cast<VkAllocationCallbacks*>(static_cast<const VkAllocationCallbacks*>(setup->allocator));
    std::u8string directory(std::strlen(graphics.runtime_directory_utf8), u8'\0');
    std::memcpy(directory.data(), graphics.runtime_directory_utf8, directory.size());
    const auto path = std::filesystem::path(directory) / L"amd_fidelityfx_vk.dll";
    if (!path.is_absolute()) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    self->module = LoadLibraryExW(path.c_str(), nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!self->module) return RSF_BACKEND_ERROR_LOAD_FAILED;
    // The SDK exports lower-case names; typed resolution stays private to this DLL.
    self->create = reinterpret_cast<PfnFfxCreateContext>(GetProcAddress(self->module, "ffxCreateContext"));
    self->destroy = reinterpret_cast<PfnFfxDestroyContext>(GetProcAddress(self->module, "ffxDestroyContext"));
    self->query = reinterpret_cast<PfnFfxQuery>(GetProcAddress(self->module, "ffxQuery"));
    self->dispatch = reinterpret_cast<PfnFfxDispatch>(GetProcAddress(self->module, "ffxDispatch"));
    self->configure = reinterpret_cast<PfnFfxConfigure>(GetProcAddress(self->module, "ffxConfigure"));
    if (!self->create || !self->destroy || !self->query || !self->dispatch || !self->configure) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
#define RSF_VK_LOAD(member, name) self->member = reinterpret_cast<PFN_##name>(device_proc_for(*self, static_cast<VkDevice>(graphics.device), #name)); if (!self->member) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT
    RSF_VK_LOAD(barrier, vkCmdPipelineBarrier); RSF_VK_LOAD(copy, vkCmdCopyImage); RSF_VK_LOAD(idle, vkDeviceWaitIdle);
#undef RSF_VK_LOAD
    auto memory = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(graphics.instance_proc(graphics.instance, "vkGetPhysicalDeviceMemoryProperties"));
    if (!memory) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    memory(static_cast<VkPhysicalDevice>(graphics.physical_device), &self->memory_properties);
    if (!self->gpu.load(*setup, false)) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    self->gpu.memory_types(self->memory_properties);
    if (!self->capture.initialize(static_cast<VkDevice>(graphics.device), graphics.device_proc, self->memory_properties)) return RSF_BACKEND_ERROR_INIT_FAILED;
    self->wsi_dispatch = reinterpret_cast<rsf_ffx_set_wsi_dispatch>(GetProcAddress(self->module, "rsfFfxSetVulkanWsiDispatch"));
    if (!self->wsi_dispatch) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    self->native_present = reinterpret_cast<PFN_vkQueuePresentKHR>(graphics.device_proc(graphics.device, "vkQueuePresentKHR"));
    self->native_acquire = reinterpret_cast<PFN_vkAcquireNextImageKHR>(graphics.device_proc(graphics.device, "vkAcquireNextImageKHR"));
    self->native_images = reinterpret_cast<PFN_vkGetSwapchainImagesKHR>(graphics.device_proc(graphics.device, "vkGetSwapchainImagesKHR"));
    self->native_create = reinterpret_cast<PFN_vkCreateSwapchainKHR>(graphics.device_proc(graphics.device, "vkCreateSwapchainKHR"));
    self->native_destroy = reinterpret_cast<PFN_vkDestroySwapchainKHR>(graphics.device_proc(graphics.device, "vkDestroySwapchainKHR"));
    const rsf_ffx_wsi_dispatch wsi{sizeof(wsi), 2, static_cast<VkDevice>(graphics.device),
        serialized_create, serialized_destroy, serialized_present, serialized_acquire, serialized_images};
    if (!self->native_create || !self->native_destroy || !self->native_present || !self->native_acquire || !self->native_images || self->wsi_dispatch(&wsi) != 0) return RSF_BACKEND_ERROR_INIT_FAILED;
    self->wsi_registered = true;
    *resources = &self->gpu;
    *out = self.release();
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

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
    const auto created = self->create(&self->swapchain, &create.header, nullptr);
    *chain = handle_value(self->chain);
    *has_context = self->swapchain ? 1u : 0u;
    return created == FFX_API_RETURN_OK ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
}
int32_t query_swapchain(rsf_ffxgvk_native* self) noexcept {
    self->functions.header.type = FFX_API_QUERY_DESC_TYPE_FGSWAPCHAIN_FUNCTIONS_VK;
    if (self->query(&self->swapchain, &self->functions.header) != FFX_API_RETURN_OK || !self->functions.pOutGetSwapchainImagesKHR ||
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
    const auto created = self->create(&self->effect, &fg.header, nullptr);
    *has_context = self->effect ? 1u : 0u;
    return created == FFX_API_RETURN_OK ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
}
int32_t query_provider(rsf_ffxgvk_native* self) noexcept {
    ffxQueryGetProviderVersion provider{}; provider.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
    if (self->query(&self->effect, &provider.header) != FFX_API_RETURN_OK || !provider.versionId) return RSF_BACKEND_ERROR_INIT_FAILED;
    self->capture.set_provider(provider.versionId, 3, true);
    return RSF_BACKEND_OK;
}
int32_t destroy_effect(rsf_ffxgvk_native* self) noexcept {
    if (self->effect && self->destroy(&self->effect, nullptr) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self->effect = nullptr;
    return RSF_BACKEND_OK;
}
void destroy_chain(rsf_ffxgvk_native* self) noexcept {
    if (self->chain && self->functions.pOutDestroySwapchainFFXAPI)
        self->functions.pOutDestroySwapchainFFXAPI(static_cast<VkDevice>(self->setup.graphics.device), self->chain, self->allocator, self->swapchain);
    self->chain = VK_NULL_HANDLE;
}
int32_t destroy_swapchain(rsf_ffxgvk_native* self) noexcept {
    if (self->swapchain && self->destroy(&self->swapchain, nullptr) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self->swapchain = nullptr;
    return RSF_BACKEND_OK;
}
int32_t images(rsf_ffxgvk_native* self, uint32_t* count, uint64_t* output) noexcept {
    return self->functions.pOutGetSwapchainImagesKHR(static_cast<VkDevice>(self->setup.graphics.device), self->chain, count,
        reinterpret_cast<VkImage*>(output));
}
int32_t acquire(rsf_ffxgvk_native* self, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index, uint64_t frame) noexcept {
    return rsf::vk_timed_call("ffx_acquire", frame, [&] {
        return self->functions.pOutAcquireNextImageKHR(static_cast<VkDevice>(self->setup.graphics.device), self->chain, timeout,
            handle<VkSemaphore>(semaphore), handle<VkFence>(fence), index);
    });
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
    return rsf::vk_timed_call("ffx_prepare_sdk", record.frame_id, [&] { return int32_t(self->dispatch(&self->effect, &data.header)); });
}
void copy(rsf_ffxgvk_native* self, void* commands, const rsf_srvk_image* source, uint64_t target, uint32_t initialized) noexcept {
    const auto list = static_cast<VkCommandBuffer>(commands);
    const auto input = handle<VkImage>(source->image), output = handle<VkImage>(target);
    transition(*self, list, input, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL);
    transition(*self, list, output, initialized ? VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL : VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
    VkImageCopy region{}; region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1}; region.extent = {self->width, self->height, 1};
    self->copy(list, input, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, output, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
    transition(*self, list, input, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
    transition(*self, list, output, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
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
    const auto result = rsf::vk_timed_call(stage, frame, [&] { return self->configure(&self->effect, &config.header); });
    return result == FFX_API_RETURN_OK ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
int32_t present(rsf_ffxgvk_native* self, const void* info, uint64_t frame, uint64_t* count) noexcept {
    const auto result = rsf::vk_timed_call("ffx_queue_present", frame, [&] {
        return self->functions.pOutQueuePresentKHR(static_cast<VkQueue>(self->setup.queues[0]), static_cast<const VkPresentInfoKHR*>(info));
    });
    if (self->functions.pOutGetLastPresentCountFFXAPI) *count = self->functions.pOutGetLastPresentCountFFXAPI(self->chain);
    return result;
}
int32_t drain(rsf_ffxgvk_native* self) noexcept {
    QueueLock lock(*self);
    ffxDispatchDescFrameGenerationSwapChainWaitForPresentsVK wait{}; wait.header.type = FFX_API_DISPATCH_DESC_TYPE_FGSWAPCHAIN_WAIT_FOR_PRESENTS_VK;
    if (self->dispatch(&self->swapchain, &wait.header) != FFX_API_RETURN_OK || self->idle(static_cast<VkDevice>(self->setup.graphics.device)) != VK_SUCCESS)
        return RSF_BACKEND_ERROR_NOT_READY;
    return RSF_BACKEND_OK;
}
void capture_flush(rsf_ffxgvk_native* self) noexcept try {
    (void)self->capture.flush();
} catch (...) {}
constexpr rsf_ffxgvk_ops operations{
    open, [](rsf_ffxgvk_native* self) { delete self; },
    create_swapchain, query_swapchain, create_effect, query_provider, destroy_effect, destroy_chain, destroy_swapchain,
    images, acquire, prepare, copy, configure, present, drain,
    [](rsf_ffxgvk_native* self, uint64_t frame) -> uint32_t { return self->capture.wants(frame) ? 1u : 0u; },
    [](rsf_ffxgvk_native* self, void* commands, const rsf_srvk_image* image, const rsf_frame_record* record, const char* role) {
        record_capture(*self, static_cast<VkCommandBuffer>(commands), *image, *record, role, record->flags & RSF_FRAME_FLAG_RESET);
    },
    capture_flush,
    [](rsf_ffxgvk_native* self, uint64_t frame) {
        self->gpu.wait_label = frame ? "ffx_slot_fence_wait" : nullptr;
        self->gpu.wait_frame = frame;
    },
    rsf::vk_timing_begin, rsf::vk_timing_end, &rsf::vkfg::platform,
};
int32_t ffx_present(rsf_ffxgvk* self, const void* info, const rsf_frame_record* source, uint32_t explicit_source) {
    uint64_t chain = 0;
    if (info) {
        const auto& vk = *static_cast<const VkPresentInfoKHR*>(info);
        if (vk.swapchainCount == 1 && vk.pSwapchains) chain = handle_value(vk.pSwapchains[0]);
    }
    return rsf_core_ffxgvk_present(self, info, chain, source, explicit_source);
}
}

extern "C" rsf_backend_result rsf_ffxgvk_create(const rsf_fgvk_setup* setup, rsf_ffxgvk** out, uint64_t* chain) {
    rsf_ffxgvk_swapchain swapchain{};
    const rsf_ffxgvk_swapchain* description = nullptr;
    if (setup && setup->struct_size >= sizeof(*setup) && setup->abi_version == RSF_FGVK_ABI_VERSION &&
        setup->graphics.struct_size >= sizeof(setup->graphics) && setup->graphics.abi_version == RSF_SRVK_ABI_VERSION &&
        setup->create_info) {
        if (*static_cast<const VkStructureType*>(setup->create_info) == VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR) {
            const auto& info = *static_cast<const VkSwapchainCreateInfoKHR*>(setup->create_info);
            swapchain = {info.imageExtent.width, info.imageExtent.height, uint32_t(info.imageFormat), uint32_t(info.imageColorSpace),
                info.imageArrayLayers, 1u, handle_value(info.oldSwapchain)};
        }
        description = &swapchain;
    }
    return rsf_core_ffxgvk_create(&operations, setup, description, out, chain);
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
extern "C" int32_t rsf_ffxgvk_present(rsf_ffxgvk* self, const void* info) { return ffx_present(self, info, nullptr, 0); }
extern "C" int32_t rsf_ffxgvk_present_source(rsf_ffxgvk* self, const void* info, const rsf_frame_record* source) {
    return ffx_present(self, info, source, 1);
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
