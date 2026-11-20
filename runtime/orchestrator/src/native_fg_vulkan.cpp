// SPDX-License-Identifier: MIT
#include "native_fg_ffx_vulkan.h"
#include "native_fg_source.h"
#include "vulkan_fg_frames.h"
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include "native_vulkan_timing.h"
#include <memory>
#include <algorithm>
#include <cmath>

#if defined(RSF_HAVE_FFX_VK)
#include <windows.h>
#include <ffx_api/ffx_framegeneration.h>
#include <ffx_api/vk/ffx_api_vk.h>
#include "vulkan_fg_resources_native.h"
#include "../../backends/common/frame_capture_vk.h"
#include "../../vulkan/fsr3/wsi-dispatch.h"
#include <array>
#include <atomic>
#include <cstring>
#include <filesystem>
#include <mutex>

namespace {
std::atomic<rsf_ffxgvk*> owner{nullptr};
struct Slot : rsf_fgvk_frame_slot {
    rsf_vkfg_fences fences{};
    rsf_vkfg_image color{};
};
}
struct rsf_ffxgvk {
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
    bool enabled = false, configured = false;
    rsf_fgvk_timeline timeline{};
    std::array<Slot, RSF_FGVK_FRAME_SLOTS> slots{};
    std::atomic<uint64_t> callbacks{0};
    rsf_fgvk_status status{sizeof(status), RSF_FGVK_ABI_VERSION};
    rsf::FrameCaptureVk capture;
    std::mutex capture_guard;
    ~rsf_ffxgvk() {
        if (effect && destroy) (void)destroy(&effect, nullptr);
        if (chain && functions.pOutDestroySwapchainFFXAPI) functions.pOutDestroySwapchainFFXAPI(
            static_cast<VkDevice>(setup.graphics.device), chain, allocator, swapchain);
        if (swapchain && destroy) (void)destroy(&swapchain, nullptr);
        if (wsi_registered && wsi_dispatch) {
            const rsf_ffx_wsi_dispatch clear{sizeof(clear), 2, static_cast<VkDevice>(setup.graphics.device), nullptr, nullptr, nullptr, nullptr, nullptr};
            (void)wsi_dispatch(&clear);
        }
        for (auto& slot : slots) {
            rsf_core_vkfg_release_image(&rsf::vkfg::platform, &gpu, &slot.color);
            rsf_core_vkfg_release_fences(&rsf::vkfg::platform, &gpu, &slot.fences);
        }
        if (owner.load() == this) owner = nullptr;
        if (module) FreeLibrary(module);
    }
};
namespace {
VkResult VKAPI_PTR serialized_create(VkDevice device, const VkSwapchainCreateInfoKHR* info, const VkAllocationCallbacks* allocator, VkSwapchainKHR* chain) {
    auto* self = owner.load();
    if (!self || self->setup.graphics.device != device || !self->native_create) return VK_ERROR_INITIALIZATION_FAILED;
    struct Lease { PFN_vkCreateSwapchainKHR call; VkDevice device; const VkSwapchainCreateInfoKHR* info; const VkAllocationCallbacks* allocator; VkSwapchainKHR* chain; };
    Lease lease{self->native_create, device, info, allocator, chain};
    // Create/destroy hooks take their own LL2 mutex. Suppress only DLSS-G;
    // never hold an outer WSI mutex across these notification hooks.
    return VkResult(rsf_slvk_without_dlssg([](void* user) -> int32_t {
        const auto& value = *static_cast<Lease*>(user); return value.call(value.device, value.info, value.allocator, value.chain);
    }, &lease));
}
void VKAPI_PTR serialized_destroy(VkDevice device, VkSwapchainKHR chain, const VkAllocationCallbacks* allocator) {
    auto* self = owner.load();
    if (!self || self->setup.graphics.device != device || !self->native_destroy) return;
    struct Lease { PFN_vkDestroySwapchainKHR call; VkDevice device; VkSwapchainKHR chain; const VkAllocationCallbacks* allocator; };
    Lease lease{self->native_destroy, device, chain, allocator};
    (void)rsf_slvk_without_dlssg([](void* user) -> int32_t {
        const auto& value = *static_cast<Lease*>(user); value.call(value.device, value.chain, value.allocator); return VK_SUCCESS;
    }, &lease);
}
VkResult VKAPI_PTR serialized_acquire(VkDevice device, VkSwapchainKHR chain, uint64_t timeout, VkSemaphore semaphore, VkFence fence, uint32_t* index) {
    const rsf::VulkanTimingScope timing("native_acquire_total", 0);
    auto* self = owner.load();
    if (!self || self->setup.graphics.device != device || !self->native_acquire) return VK_ERROR_INITIALIZATION_FAILED;
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
    auto* self = owner.load();
    if (!self || self->setup.graphics.device != device || !self->native_images) return VK_ERROR_INITIALIZATION_FAILED;
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
    auto* self = owner.load();
    if (!self || !self->native_present) return VK_ERROR_INITIALIZATION_FAILED;
    struct Lease { PFN_vkQueuePresentKHR call; VkQueue queue; const VkPresentInfoKHR* info; } lease{self->native_present, queue, info};
    return VkResult(rsf_slvk_without_dlssg([](void* user) -> int32_t {
        const auto& value = *static_cast<Lease*>(user);
        return rsf::vk_timed_call("native_queue_present", 0, [&] {
            return rsf_latencyvk_queue_present(reinterpret_cast<rsf_vk_function>(value.call), value.queue, value.info);
        });
    }, &lease));
}
PFN_vkVoidFunction VKAPI_PTR device_proc(VkDevice device, const char* name) {
    auto* self = owner.load();
    if (!self || self->setup.graphics.device != device) return nullptr;
    auto proc = self->setup.graphics.device_proc;
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
VkResult graphics_submit(uint32_t count, const VkSubmitInfo* submits, VkFence fence) {
    auto* self = owner.load();
    return self ? VkResult(self->setup.graphics_submit(self->setup.queue_user, count, submits, reinterpret_cast<uint64_t>(fence))) : VK_ERROR_DEVICE_LOST;
}
struct QueueLock {
    rsf_ffxgvk& self;
    explicit QueueLock(rsf_ffxgvk& value) : self(value) { self.setup.lock_queues(self.setup.queue_user); }
    ~QueueLock() { self.setup.unlock_queues(self.setup.queue_user); }
};
void transition(rsf_ffxgvk& self, VkCommandBuffer commands, VkImage image, VkImageLayout from, VkImageLayout to) {
    VkImageMemoryBarrier data{}; data.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
    data.oldLayout = from; data.newLayout = to;
    data.srcAccessMask = from == VK_IMAGE_LAYOUT_UNDEFINED ? 0 : VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    data.dstAccessMask = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    data.srcQueueFamilyIndex = data.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    data.image = image; data.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    self.barrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &data);
}
FfxApiResource resource(const rsf_srvk_image& image) {
    FfxApiResource value{}; value.resource = reinterpret_cast<void*>(static_cast<uintptr_t>(image.image));
    value.description.type = FFX_API_RESOURCE_TYPE_TEXTURE2D;
    value.description.width = image.width; value.description.height = image.height; value.description.depth = value.description.mipCount = 1;
    value.description.format = ffxApiGetSurfaceFormatVK(VkFormat(image.format));
    value.description.usage = FFX_API_RESOURCE_USAGE_READ_ONLY; value.state = FFX_API_RESOURCE_STATE_COMPUTE_READ;
    return value;
}
ffxReturnCode_t generate(ffxDispatchDescFrameGeneration* data, void* userdata) {
    auto& self = *static_cast<rsf_ffxgvk*>(userdata);
    if (!data) return FFX_API_RETURN_ERROR_PARAMETER;
    const auto& slot = self.slots[data->frameID % self.slots.size()];
    if (slot.record.frame_id != data->frameID || !slot.submitted_prepare || !slot.submitted_hudless) return FFX_API_RETURN_ERROR_PARAMETER;
    data->reset |= (slot.record.flags & RSF_FRAME_FLAG_RESET) != 0;
    auto result = self.dispatch(&self.effect, &data->header);
    if (result == FFX_API_RETURN_OK) {
        ++self.callbacks;
        std::lock_guard lock(self.capture_guard);
        if (self.capture.wants(data->frameID)) for (uint32_t i = 0; i < data->numGeneratedFrames && i < 4; ++i) {
            const auto& source = data->outputs[i];
            rsf_srvk_image image{self.setup.graphics.device, reinterpret_cast<uint64_t>(source.resource),
                uint32_t(VK_FORMAT_R8G8B8A8_UNORM), source.description.width, source.description.height, VK_IMAGE_LAYOUT_GENERAL, 1};
            // The generated output uses the actual swapchain channel order.
            if (source.description.format == FFX_API_SURFACE_FORMAT_B8G8R8A8_UNORM) image.format = VK_FORMAT_B8G8R8A8_UNORM;
            (void)self.capture.record(static_cast<VkCommandBuffer>(data->commandList), image, slot.record, "fg-generated-before-ui", data->reset, false);
        }
    }
    return result;
}
bool valid_record(const rsf_ffxgvk& self, const rsf_fgvk_frame* frame) {
    if (!frame || frame->struct_size < sizeof(*frame) || frame->abi_version != RSF_FGVK_ABI_VERSION || !frame->record) return false;
    const auto& record = *frame->record;
    if (record.struct_size < sizeof(record) || record.abi_version != RSF_GAME_FRAME_ABI_VERSION ||
        record.camera.struct_size < sizeof(record.camera) || record.camera.abi_version != RSF_GAME_CAMERA_ABI_VERSION) return false;
    const auto& camera = record.camera;
    bool finite = true;
    for (const auto* matrix : {camera.view_to_clip, camera.clip_to_view, camera.view_to_world, camera.world_to_view, camera.clip_to_previous_clip})
        for (uint32_t i = 0; i < 16; ++i) finite &= std::isfinite(matrix[i]);
    return finite && record.struct_size >= sizeof(record) && record.abi_version == RSF_GAME_FRAME_ABI_VERSION &&
        record.camera.struct_size >= sizeof(record.camera) && record.camera.abi_version == RSF_GAME_CAMERA_ABI_VERSION &&
        record.session_id && record.frame_id && record.render_width && record.render_height &&
        record.render_width <= self.width && record.render_height <= self.height && record.output_width == self.width && record.output_height == self.height &&
        camera.render_width == record.render_width && camera.render_height == record.render_height && camera.output_width == self.width && camera.output_height == self.height &&
        std::isfinite(record.frame_time_ms) && record.frame_time_ms > 0 && std::isfinite(camera.near_plane) && camera.near_plane > 0 &&
        std::isfinite(camera.far_plane) && camera.far_plane >= 0 && (self.setup.graphics.depth_infinite || camera.far_plane > camera.near_plane) &&
        std::isfinite(camera.vertical_fov_radians) && camera.vertical_fov_radians > 0 && camera.vertical_fov_radians < 3.141593f &&
        std::isfinite(camera.jitter_pixels[0]) && std::isfinite(camera.jitter_pixels[1]);
}
rsf_backend_result configure_frame(rsf_ffxgvk& self, const Slot* slot) {
    ffxConfigureDescFrameGeneration config{}; config.header.type = FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION;
    config.swapChain = reinterpret_cast<void*>(self.chain);
    config.frameGenerationEnabled = slot != nullptr;
    config.allowAsyncWorkloads = false;
    config.frameGenerationCallback = generate; config.frameGenerationCallbackUserContext = &self;
    config.frameID = slot ? slot->record.frame_id : self.timeline.last_prepared.frame_id;
    config.generationRect = {0, 0, int32_t(self.width), int32_t(self.height)};
    if (slot) {
        rsf_srvk_image color{self.setup.graphics.device, slot->color.image, VK_FORMAT_R8G8B8A8_UNORM,
            self.width, self.height, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
        config.HUDLessColor = resource(color);
    }
    const char* timing_stage = slot ? (self.configured ? "ffx_configure_update" : "ffx_configure_enable") :
        (self.configured ? "ffx_configure_disable" : "ffx_configure_disabled");
    const auto result = rsf::vk_timed_call(timing_stage, config.frameID, [&] { return self.configure(&self.effect, &config.header); });
    if (result != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self.configured = slot != nullptr; return RSF_BACKEND_OK;
}
}
#else
struct rsf_ffxgvk {};
#endif

extern "C" rsf_backend_result rsf_ffxgvk_create(const rsf_fgvk_setup* setup, rsf_ffxgvk** out, uint64_t* chain) try {
    if (!out || !chain) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr; *chain = 0;
    if (!setup || setup->struct_size < sizeof(*setup)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_FGVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    const auto& graphics = setup->graphics;
    if (graphics.struct_size < sizeof(graphics)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (graphics.abi_version != RSF_SRVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (graphics.struct_size < sizeof(graphics) || !graphics.device || !graphics.physical_device || !graphics.instance || !graphics.device_proc ||
        !graphics.instance_proc || !graphics.runtime_directory_utf8 || !setup->create_info || !setup->graphics_submit || !setup->lock_queues || !setup->unlock_queues ||
        graphics.depth_inverted > 1 || graphics.depth_infinite > 1 || graphics.motion_jittered > 1)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    for (uint32_t i = 0; i < 4; ++i) {
        if (!setup->queues[i]) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        for (uint32_t j = 0; j < i; ++j) if (setup->queues[i] == setup->queues[j]) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
#if defined(RSF_HAVE_FFX_VK)
    const auto& info = *static_cast<const VkSwapchainCreateInfoKHR*>(setup->create_info);
    if (info.oldSwapchain || info.sType != VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR || !info.imageExtent.width || !info.imageExtent.height ||
        info.imageArrayLayers != 1 || info.imageColorSpace != VK_COLOR_SPACE_SRGB_NONLINEAR_KHR ||
        !(info.imageFormat == VK_FORMAT_R8G8B8A8_UNORM || info.imageFormat == VK_FORMAT_B8G8R8A8_UNORM)) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    auto self = std::make_unique<rsf_ffxgvk>(); self->setup = *setup;
    rsf_ffxgvk* expected = nullptr;
    if (!owner.compare_exchange_strong(expected, self.get())) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    self->width = info.imageExtent.width; self->height = info.imageExtent.height;
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
#define RSF_VK_LOAD(member, name) self->member = reinterpret_cast<PFN_##name>(device_proc(static_cast<VkDevice>(graphics.device), #name)); if (!self->member) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT
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
    ffxCreateContextDescFrameGenerationSwapChainVK create{}; create.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FGSWAPCHAIN_VK;
    create.physicalDevice = static_cast<VkPhysicalDevice>(graphics.physical_device); create.device = static_cast<VkDevice>(graphics.device);
    create.swapchain = &self->chain; create.allocator = self->allocator; create.createInfo = info;
    create.gameQueue = {static_cast<VkQueue>(setup->queues[0]), setup->queue_family, graphics_submit};
    create.asyncComputeQueue = {static_cast<VkQueue>(setup->queues[1]), setup->queue_family, nullptr};
    create.presentQueue = {static_cast<VkQueue>(setup->queues[2]), setup->queue_family, nullptr};
    create.imageAcquireQueue = {static_cast<VkQueue>(setup->queues[3]), setup->queue_family, nullptr};
    if (self->create(&self->swapchain, &create.header, nullptr) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_INIT_FAILED;
    self->functions.header.type = FFX_API_QUERY_DESC_TYPE_FGSWAPCHAIN_FUNCTIONS_VK;
    if (self->query(&self->swapchain, &self->functions.header) != FFX_API_RETURN_OK || !self->functions.pOutGetSwapchainImagesKHR ||
        !self->functions.pOutAcquireNextImageKHR || !self->functions.pOutQueuePresentKHR || !self->functions.pOutDestroySwapchainFFXAPI)
        return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    ffxCreateBackendVKDesc backend{}; backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK;
    backend.vkDevice = create.device; backend.vkPhysicalDevice = create.physicalDevice; backend.vkDeviceProcAddr = device_proc;
    ffxCreateContextDescFrameGenerationHudless hudless{}; hudless.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION_HUDLESS;
    hudless.header.pNext = &backend.header; hudless.hudlessBackBufferFormat = FFX_API_SURFACE_FORMAT_R8G8B8A8_UNORM;
    ffxCreateContextDescFrameGeneration fg{}; fg.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION; fg.header.pNext = &hudless.header;
    fg.flags = (graphics.depth_inverted ? FFX_FRAMEGENERATION_ENABLE_DEPTH_INVERTED : 0u) |
        (graphics.depth_infinite ? FFX_FRAMEGENERATION_ENABLE_DEPTH_INFINITE : 0u) |
        (graphics.motion_jittered ? FFX_FRAMEGENERATION_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION : 0u);
    fg.displaySize = fg.maxRenderSize = {self->width, self->height}; fg.backBufferFormat = ffxApiGetSurfaceFormatVK(info.imageFormat);
    if (self->create(&self->effect, &fg.header, nullptr) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_INIT_FAILED;
    ffxQueryGetProviderVersion provider{}; provider.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
    if (self->query(&self->effect, &provider.header) != FFX_API_RETURN_OK || !provider.versionId) return RSF_BACKEND_ERROR_INIT_FAILED;
    self->capture.set_provider(provider.versionId, 3, true);
    self->status.available = 1; self->status.result = RSF_BACKEND_OK;
    *chain = reinterpret_cast<uint64_t>(self->chain); *out = self.release(); return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

extern "C" int32_t rsf_ffxgvk_images(rsf_ffxgvk* self, uint32_t* count, uint64_t* images) {
#if defined(RSF_HAVE_FFX_VK)
    if (!self || !count) return VK_ERROR_INITIALIZATION_FAILED;
    const auto query = self->functions.pOutGetSwapchainImagesKHR;
    const auto device = static_cast<VkDevice>(self->setup.graphics.device);
    if (!images) return query(device, self->chain, count, nullptr);
    // FidelityFX FrameInterpolationSwapChainVK::getSwapchainImages leaves an
    // oversized capacity unchanged after filling its smaller image set. Query
    // the real count first so callers never consume uninitialized tail handles.
    const uint32_t capacity = *count;
    uint32_t available = 0;
    const auto result = query(device, self->chain, &available, nullptr);
    if (result != VK_SUCCESS) { *count = 0; return result; }
    *count = (std::min)(capacity, available);
    const auto filled = query(device, self->chain, count, reinterpret_cast<VkImage*>(images));
    if (*count > capacity || *count > available) { *count = 0; return VK_ERROR_INITIALIZATION_FAILED; }
    return filled == VK_SUCCESS && capacity < available ? VK_INCOMPLETE : filled;
#else
    (void)self; (void)count; (void)images; return -3;
#endif
}
extern "C" int32_t rsf_ffxgvk_acquire(rsf_ffxgvk* self, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
#if defined(RSF_HAVE_FFX_VK)
    if (!self || !index) return VK_ERROR_INITIALIZATION_FAILED;
    return rsf::vk_timed_call("ffx_acquire", self->timeline.last_prepared.frame_id, [&] {
        return self->functions.pOutAcquireNextImageKHR(static_cast<VkDevice>(self->setup.graphics.device), self->chain, timeout,
            reinterpret_cast<VkSemaphore>(static_cast<uintptr_t>(semaphore)), reinterpret_cast<VkFence>(static_cast<uintptr_t>(fence)), index);
    });
#else
    (void)self; (void)timeout; (void)semaphore; (void)fence; (void)index; return -3;
#endif
}
extern "C" rsf_backend_result rsf_ffxgvk_enable(rsf_ffxgvk* self, uint32_t enabled) {
    if (!self || enabled > 1) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_FFX_VK)
    self->enabled = enabled != 0; return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
extern "C" rsf_backend_result rsf_ffxgvk_prepare(rsf_ffxgvk* self, void* commands, const rsf_fgvk_frame* frame) try {
    if (!self || !commands || !frame || frame->struct_size < sizeof(*frame)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (frame->abi_version != RSF_FGVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if defined(RSF_HAVE_FFX_VK)
    if (!valid_record(*self, frame)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const rsf::VulkanTimingScope timing("ffx_prepare_total", frame->record->frame_id);
    if (!self->enabled && !self->capture.wants(frame->record->frame_id)) return RSF_BACKEND_OK;
    const auto& record = *frame->record;
    if (frame->depth.device != self->setup.graphics.device || frame->motion.device != self->setup.graphics.device ||
        !frame->depth.image || !frame->motion.image || frame->depth.format != VK_FORMAT_R32_SFLOAT || frame->motion.format != VK_FORMAT_R16G16_SFLOAT ||
        frame->depth.width != record.render_width || frame->depth.height != record.render_height ||
        frame->motion.width != record.render_width || frame->motion.height != record.render_height ||
        frame->depth.layout != VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL || frame->motion.layout != VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL ||
        !std::isfinite(frame->motion_scale_x) || !std::isfinite(frame->motion_scale_y) || !frame->motion_scale_x || !frame->motion_scale_y ||
        !std::isfinite(record.frame_time_ms) || record.frame_time_ms <= 0 || record.camera.depth_inverted != self->setup.graphics.depth_inverted ||
        record.camera.motion_jittered != self->setup.graphics.motion_jittered) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& slot = self->slots[record.frame_id % self->slots.size()];
    if (!rsf_core_fgvk_can_prepare(&slot)) return RSF_BACKEND_ERROR_NOT_READY;
    self->gpu.wait_label = "ffx_slot_fence_wait";
    self->gpu.wait_frame = record.frame_id;
    const auto fences = rsf_core_vkfg_prepare_fences(&rsf::vkfg::platform, &self->gpu, &slot, &slot.fences);
    self->gpu.wait_label = nullptr;
    if (fences != RSF_BACKEND_OK) return fences;
    rsf_core_fgvk_prepare_begin(&self->timeline, &slot, &record, commands);
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
    // Dispatch can record GPU work before reporting an error. Submission still
    // owns that recording even when it never becomes an accepted FG input.
    slot.recorded_prepare = true;
    if (self->enabled) {
        if (rsf::vk_timed_call("ffx_prepare_sdk", record.frame_id, [&] {
            return self->dispatch(&self->effect, &data.header);
        }) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
        slot.prepared = true; ++self->status.prepared;
    }
    rsf_core_fgvk_prepare_accepted(&self->timeline, &slot);
    { std::lock_guard lock(self->capture_guard);
        (void)self->capture.record(static_cast<VkCommandBuffer>(commands), frame->depth, record, "fg-depth", record.flags & RSF_FRAME_FLAG_RESET);
        (void)self->capture.record(static_cast<VkCommandBuffer>(commands), frame->motion, record, "fg-motion-uv", record.flags & RSF_FRAME_FLAG_RESET); }
    return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" rsf_backend_result rsf_ffxgvk_hudless(rsf_ffxgvk* self, void* commands, const rsf_fgvk_frame* frame) try {
    if (!self || !commands || !frame || frame->struct_size < sizeof(*frame)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (frame->abi_version != RSF_FGVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if defined(RSF_HAVE_FFX_VK)
    if (!valid_record(*self, frame)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!self->enabled && !self->capture.wants(frame->record->frame_id)) return RSF_BACKEND_OK;
    const auto& record = *frame->record; auto& slot = self->slots[record.frame_id % self->slots.size()];
    if (frame->hudless.device != self->setup.graphics.device || !frame->hudless.image || !frame->hudless.copy_source ||
        frame->hudless.format != VK_FORMAT_R8G8B8A8_UNORM || frame->hudless.width != self->width ||
        frame->hudless.height != self->height || frame->hudless.layout != VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL ||
        !rsf_core_fgvk_hudless_begin(&slot, &record, commands))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!rsf_core_vkfg_ensure_image(&rsf::vkfg::platform, &self->gpu, &slot.color, self->width, self->height,
        VK_FORMAT_R8G8B8A8_UNORM, 0)) return RSF_BACKEND_ERROR_INIT_FAILED;
    auto list = static_cast<VkCommandBuffer>(commands);
    slot.recorded_hudless = true;
    auto input = reinterpret_cast<VkImage>(static_cast<uintptr_t>(frame->hudless.image));
    transition(*self, list, input, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL);
    transition(*self, list, rsf::vkfg::image_handle(slot.color), slot.color.initialized ? VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL : VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
    VkImageCopy region{}; region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1}; region.extent = {self->width, self->height, 1};
    self->copy(list, input, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, rsf::vkfg::image_handle(slot.color), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
    transition(*self, list, input, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
    transition(*self, list, rsf::vkfg::image_handle(slot.color), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL); slot.color.initialized = true;
    slot.hudless = true; if (self->enabled) ++self->status.tagged;
    rsf_srvk_image image{self->setup.graphics.device, slot.color.image, VK_FORMAT_R8G8B8A8_UNORM,
        self->width, self->height, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
    { std::lock_guard lock(self->capture_guard); (void)self->capture.record(list, image, record, "fg-hudless-color", record.flags & RSF_FRAME_FLAG_RESET); }
    return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" void rsf_ffxgvk_submitted(rsf_ffxgvk* self, void* const* commands, uint32_t count) {
#if defined(RSF_HAVE_FFX_VK)
    if (!self || !commands) return;
    std::array<rsf_fgvk_frame_slot*, RSF_FGVK_FRAME_SLOTS> states{};
    for (size_t i = 0; i < states.size(); ++i) states[i] = &self->slots[i];
    rsf_fgvk_submit_plan plan{};
    rsf_core_fgvk_submitted(states.data(), commands, count, &plan);
    for (size_t i = 0; i < states.size(); ++i) {
        auto& slot = self->slots[i];
        const auto bit = 1u << i;
        if (plan.discarded_hudless & bit) slot.color.initialized = false;
        if (!(plan.fences & bit)) continue;
        rsf_core_vkfg_fence_submission(&rsf::vkfg::platform, &self->gpu, &slot, &slot.fences, 0);
    }
#else
    (void)self; (void)commands; (void)count;
#endif
}
static int32_t ffx_present(rsf_ffxgvk* self, const void* present_info, const rsf_frame_record* source, bool explicit_source) try {
    if (explicit_source && !rsf::fg_present_identity_valid(source)) return -3;
#if defined(RSF_HAVE_FFX_VK)
    if (!self || !present_info) return VK_ERROR_INITIALIZATION_FAILED;
    const uint64_t source_frame = rsf::fg_present_frame(self->timeline.last_prepared.frame_id, source, explicit_source);
    const rsf::VulkanTimingScope timing("ffx_present_total", source_frame);
    const auto& info = *static_cast<const VkPresentInfoKHR*>(present_info);
    if (info.swapchainCount != 1 || !info.pSwapchains || info.pSwapchains[0] != self->chain) return VK_ERROR_INITIALIZATION_FAILED;
    auto& slot = self->slots[source_frame % self->slots.size()];
    const bool identity = rsf::fg_present_identity_matches(slot.record, source, explicit_source);
    // A camera reset discards dispatch history through generate(), rather than
    // disabling the presenter and draining its queues and pacing threads. Keep
    // activation stable while this frame's submitted input leases are valid.
    const bool valid = rsf_core_fgvk_present_candidate(&self->timeline, &slot, source_frame, identity, self->enabled) != 0;
    const auto configured = configure_frame(*self, valid ? &slot : nullptr);
    if (configured != RSF_BACKEND_OK) { self->status.result = configured; return VK_ERROR_INITIALIZATION_FAILED; }
    const auto result = rsf::vk_timed_call("ffx_queue_present", source_frame, [&] {
        return self->functions.pOutQueuePresentKHR(static_cast<VkQueue>(self->setup.queues[0]), &info);
    });
    if (valid && (result == VK_SUCCESS || result == VK_SUBOPTIMAL_KHR)) {
        rsf_core_vkfg_fence_submission(&rsf::vkfg::platform, &self->gpu, &slot, &slot.fences, 1);
    }
    rsf_core_fgvk_presented(&self->timeline, &slot, source_frame, identity);
    self->status.source_frame = explicit_source && !valid ? 0 : source_frame;
    self->status.enabled = valid ? 1u : 0u; self->status.result = result == VK_SUCCESS || result == VK_SUBOPTIMAL_KHR ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
    if (self->functions.pOutGetLastPresentCountFFXAPI) self->status.last_present_count = self->functions.pOutGetLastPresentCountFFXAPI(self->chain);
    return result;
#else
    (void)self; (void)present_info; return -3;
#endif
} catch (...) { return -3; }
extern "C" int32_t rsf_ffxgvk_present(rsf_ffxgvk* self, const void* info) { return ffx_present(self, info, nullptr, false); }
extern "C" int32_t rsf_ffxgvk_present_source(rsf_ffxgvk* self, const void* info, const rsf_frame_record* source) {
    return ffx_present(self, info, source, true);
}
extern "C" rsf_backend_result rsf_ffxgvk_get_status(rsf_ffxgvk* self, rsf_fgvk_status* status) {
    if (!self || !status || status->struct_size < sizeof(*status)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (status->abi_version != RSF_FGVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if defined(RSF_HAVE_FFX_VK)
    *status = self->status; status->generated_callbacks = self->callbacks.load(); return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
extern "C" rsf_backend_result rsf_ffxgvk_drain(rsf_ffxgvk* self) try {
    if (!self) return RSF_BACKEND_OK;
#if defined(RSF_HAVE_FFX_VK)
    // Device idle cannot cover commands the host has not submitted yet.
    for (const auto& slot : self->slots)
        if (rsf_core_fgvk_unsubmitted(&slot))
            return RSF_BACKEND_ERROR_NOT_READY;
    QueueLock lock(*self);
    ffxDispatchDescFrameGenerationSwapChainWaitForPresentsVK wait{}; wait.header.type = FFX_API_DISPATCH_DESC_TYPE_FGSWAPCHAIN_WAIT_FOR_PRESENTS_VK;
    if (self->dispatch(&self->swapchain, &wait.header) != FFX_API_RETURN_OK || self->idle(static_cast<VkDevice>(self->setup.graphics.device)) != VK_SUCCESS)
        return RSF_BACKEND_ERROR_NOT_READY;
    { std::lock_guard capture_lock(self->capture_guard); (void)self->capture.flush(); }
    return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" rsf_backend_result rsf_ffxgvk_destroy(rsf_ffxgvk* self) try {
    if (!self) return RSF_BACKEND_OK;
#if defined(RSF_HAVE_FFX_VK)
    for (const auto& slot : self->slots)
        if (rsf_core_fgvk_unsubmitted(&slot))
            return RSF_BACKEND_ERROR_NOT_READY;
    if (self->effect && configure_frame(*self, nullptr) != RSF_BACKEND_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    if (self->chain && rsf_ffxgvk_drain(self) != RSF_BACKEND_OK) return RSF_BACKEND_ERROR_NOT_READY;
    if (self->effect && self->destroy(&self->effect, nullptr) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self->effect = nullptr;
    if (self->chain) self->functions.pOutDestroySwapchainFFXAPI(static_cast<VkDevice>(self->setup.graphics.device), self->chain, self->allocator, self->swapchain);
    self->chain = VK_NULL_HANDLE;
    if (self->swapchain && self->destroy(&self->swapchain, nullptr) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self->swapchain = nullptr;
#endif
    delete self; return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
