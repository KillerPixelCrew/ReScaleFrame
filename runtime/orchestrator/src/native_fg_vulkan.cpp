// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_fg_vulkan.h>
#include <memory>
#include <cmath>

#if defined(RSF_HAVE_FFX_VK)
#include <windows.h>
#include <ffx_api/ffx_framegeneration.h>
#include <ffx_api/vk/ffx_api_vk.h>
#include "../../backends/common/frame_capture_vk.h"
#include <array>
#include <atomic>
#include <cstring>
#include <filesystem>
#include <mutex>

namespace {
std::atomic<rsf_fgvk*> owner{nullptr};
struct Image { VkImage image = VK_NULL_HANDLE; VkDeviceMemory memory = VK_NULL_HANDLE; bool initialized = false; };
struct Slot {
    rsf_frame_record record{};
    void* prepare_commands = nullptr;
    void* hudless_commands = nullptr;
    bool guides = false, prepared = false, hudless = false, submitted_prepare = false, submitted_hudless = false;
    VkFence source_completion = VK_NULL_HANDLE, consumer_completion = VK_NULL_HANDLE;
    bool source_pending = false, consumer_pending = false, completion_unknown = false;
    Image color;
};
}
struct rsf_fgvk {
    rsf_fgvk_setup setup{};
    HMODULE module = nullptr;
    ffxContext swapchain = nullptr, effect = nullptr;
    VkSwapchainKHR chain = VK_NULL_HANDLE;
    PfnFfxCreateContext create = nullptr;
    PfnFfxDestroyContext destroy = nullptr;
    PfnFfxQuery query = nullptr;
    PfnFfxDispatch dispatch = nullptr;
    PfnFfxConfigure configure = nullptr;
    ffxQueryDescSwapchainReplacementFunctionsVK functions{};
    PFN_vkCreateImage create_image = nullptr;
    PFN_vkDestroyImage destroy_image = nullptr;
    PFN_vkGetImageMemoryRequirements requirements = nullptr;
    PFN_vkAllocateMemory allocate = nullptr;
    PFN_vkFreeMemory free_memory = nullptr;
    PFN_vkBindImageMemory bind = nullptr;
    PFN_vkCmdPipelineBarrier barrier = nullptr;
    PFN_vkCmdCopyImage copy = nullptr;
    PFN_vkDeviceWaitIdle idle = nullptr;
    PFN_vkCreateFence create_fence = nullptr;
    PFN_vkDestroyFence destroy_fence = nullptr;
    PFN_vkResetFences reset_fences = nullptr;
    PFN_vkWaitForFences wait_fences = nullptr;
    VkPhysicalDeviceMemoryProperties memory_properties{};
    VkAllocationCallbacks* allocator = nullptr;
    uint32_t width = 0, height = 0;
    bool enabled = false, configured = false;
    uint64_t latest = 0, presented = 0;
    std::array<Slot, 6> slots;
    std::atomic<uint64_t> callbacks{0};
    rsf_fgvk_status status{sizeof(status), RSF_FGVK_ABI_VERSION};
    rsf::FrameCaptureVk capture;
    std::mutex capture_guard;
    ~rsf_fgvk() {
        if (effect && destroy) (void)destroy(&effect, nullptr);
        if (chain && functions.pOutDestroySwapchainFFXAPI) functions.pOutDestroySwapchainFFXAPI(
            static_cast<VkDevice>(setup.graphics.device), chain, allocator, swapchain);
        if (swapchain && destroy) (void)destroy(&swapchain, nullptr);
        for (auto& slot : slots) {
            if (slot.color.image && destroy_image) destroy_image(static_cast<VkDevice>(setup.graphics.device), slot.color.image, nullptr);
            if (slot.color.memory && free_memory) free_memory(static_cast<VkDevice>(setup.graphics.device), slot.color.memory, nullptr);
            if (slot.source_completion && destroy_fence) destroy_fence(static_cast<VkDevice>(setup.graphics.device), slot.source_completion, nullptr);
            if (slot.consumer_completion && destroy_fence) destroy_fence(static_cast<VkDevice>(setup.graphics.device), slot.consumer_completion, nullptr);
        }
        if (owner.load() == this) owner = nullptr;
        if (module) FreeLibrary(module);
    }
};
namespace {
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
    rsf_fgvk& self;
    explicit QueueLock(rsf_fgvk& value) : self(value) { self.setup.lock_queues(self.setup.queue_user); }
    ~QueueLock() { self.setup.unlock_queues(self.setup.queue_user); }
};
void transition(rsf_fgvk& self, VkCommandBuffer commands, VkImage image, VkImageLayout from, VkImageLayout to) {
    VkImageMemoryBarrier data{}; data.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
    data.oldLayout = from; data.newLayout = to;
    data.srcAccessMask = from == VK_IMAGE_LAYOUT_UNDEFINED ? 0 : VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    data.dstAccessMask = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    data.srcQueueFamilyIndex = data.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    data.image = image; data.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    self.barrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &data);
}
bool allocate_color(rsf_fgvk& self, Image& image) {
    auto device = static_cast<VkDevice>(self.setup.graphics.device);
    VkImageCreateInfo info{}; info.sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO; info.imageType = VK_IMAGE_TYPE_2D;
    info.extent = {self.width, self.height, 1}; info.format = VK_FORMAT_R8G8B8A8_UNORM;
    info.mipLevels = info.arrayLayers = 1; info.samples = VK_SAMPLE_COUNT_1_BIT;
    info.usage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
    if (self.create_image(device, &info, nullptr, &image.image) != VK_SUCCESS) return false;
    VkMemoryRequirements required{}; self.requirements(device, image.image, &required);
    uint32_t type = UINT32_MAX;
    for (uint32_t i = 0; i < self.memory_properties.memoryTypeCount; ++i) if ((required.memoryTypeBits & (1u << i)) &&
        (self.memory_properties.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT)) { type = i; break; }
    if (type == UINT32_MAX) return false;
    VkMemoryAllocateInfo allocation{}; allocation.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO;
    allocation.allocationSize = required.size; allocation.memoryTypeIndex = type;
    return self.allocate(device, &allocation, nullptr, &image.memory) == VK_SUCCESS && self.bind(device, image.image, image.memory, 0) == VK_SUCCESS;
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
    auto& self = *static_cast<rsf_fgvk*>(userdata);
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
            (void)self.capture.record(static_cast<VkCommandBuffer>(data->commandList), image, slot.record, "fg-generated-before-ui", data->reset);
        }
    }
    return result;
}
bool valid_record(const rsf_fgvk& self, const rsf_fgvk_frame* frame) {
    if (!frame || frame->struct_size < sizeof(*frame) || frame->abi_version != RSF_FGVK_ABI_VERSION || !frame->record) return false;
    const auto& record = *frame->record;
    if (record.struct_size < sizeof(record) || record.abi_version != RSF_GAME_FRAME_ABI_VERSION ||
        record.camera.struct_size < sizeof(record.camera) || record.camera.abi_version != RSF_GAME_FRAME_ABI_VERSION) return false;
    const auto& camera = record.camera;
    bool finite = true;
    for (const auto* matrix : {camera.view_to_clip, camera.clip_to_view, camera.view_to_world, camera.world_to_view, camera.clip_to_previous_clip})
        for (uint32_t i = 0; i < 16; ++i) finite &= std::isfinite(matrix[i]);
    return finite && record.struct_size >= sizeof(record) && record.abi_version == RSF_GAME_FRAME_ABI_VERSION &&
        record.camera.struct_size >= sizeof(record.camera) && record.camera.abi_version == RSF_GAME_FRAME_ABI_VERSION &&
        record.session_id && record.frame_id && record.render_width && record.render_height &&
        record.render_width <= self.width && record.render_height <= self.height && record.output_width == self.width && record.output_height == self.height &&
        camera.render_width == record.render_width && camera.render_height == record.render_height && camera.output_width == self.width && camera.output_height == self.height &&
        std::isfinite(record.frame_time_ms) && record.frame_time_ms > 0 && std::isfinite(camera.near_plane) && camera.near_plane > 0 &&
        std::isfinite(camera.far_plane) && camera.far_plane >= 0 && (self.setup.graphics.depth_infinite || camera.far_plane > camera.near_plane) &&
        std::isfinite(camera.vertical_fov_radians) && camera.vertical_fov_radians > 0 && camera.vertical_fov_radians < 3.141593f &&
        std::isfinite(camera.jitter_pixels[0]) && std::isfinite(camera.jitter_pixels[1]);
}
rsf_backend_result configure_frame(rsf_fgvk& self, const Slot* slot) {
    ffxConfigureDescFrameGeneration config{}; config.header.type = FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION;
    config.swapChain = reinterpret_cast<void*>(self.chain);
    config.frameGenerationEnabled = slot != nullptr;
    config.allowAsyncWorkloads = false;
    config.frameGenerationCallback = generate; config.frameGenerationCallbackUserContext = &self;
    config.frameID = slot ? slot->record.frame_id : self.latest;
    config.generationRect = {0, 0, int32_t(self.width), int32_t(self.height)};
    if (slot) {
        rsf_srvk_image color{self.setup.graphics.device, reinterpret_cast<uint64_t>(slot->color.image), VK_FORMAT_R8G8B8A8_UNORM,
            self.width, self.height, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
        config.HUDLessColor = resource(color);
    }
    const auto result = self.configure(&self.effect, &config.header);
    if (result != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self.configured = slot != nullptr; return RSF_BACKEND_OK;
}
}
#else
struct rsf_fgvk {};
#endif

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fgvk_create(const rsf_fgvk_setup* setup, rsf_fgvk** out, uint64_t* chain) try {
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
    auto self = std::make_unique<rsf_fgvk>(); self->setup = *setup;
    rsf_fgvk* expected = nullptr;
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
    RSF_VK_LOAD(create_image, vkCreateImage); RSF_VK_LOAD(destroy_image, vkDestroyImage); RSF_VK_LOAD(requirements, vkGetImageMemoryRequirements);
    RSF_VK_LOAD(allocate, vkAllocateMemory); RSF_VK_LOAD(free_memory, vkFreeMemory); RSF_VK_LOAD(bind, vkBindImageMemory);
    RSF_VK_LOAD(barrier, vkCmdPipelineBarrier); RSF_VK_LOAD(copy, vkCmdCopyImage); RSF_VK_LOAD(idle, vkDeviceWaitIdle);
    RSF_VK_LOAD(create_fence, vkCreateFence); RSF_VK_LOAD(destroy_fence, vkDestroyFence);
    RSF_VK_LOAD(reset_fences, vkResetFences); RSF_VK_LOAD(wait_fences, vkWaitForFences);
#undef RSF_VK_LOAD
    auto memory = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(graphics.instance_proc(graphics.instance, "vkGetPhysicalDeviceMemoryProperties"));
    if (!memory) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    memory(static_cast<VkPhysicalDevice>(graphics.physical_device), &self->memory_properties);
    if (!self->capture.initialize(static_cast<VkDevice>(graphics.device), graphics.device_proc, self->memory_properties)) return RSF_BACKEND_ERROR_INIT_FAILED;
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

extern "C" RSF_RUNTIME_API int32_t rsf_fgvk_images(rsf_fgvk* self, uint32_t* count, uint64_t* images) {
#if defined(RSF_HAVE_FFX_VK)
    return self && count ? self->functions.pOutGetSwapchainImagesKHR(static_cast<VkDevice>(self->setup.graphics.device), self->chain, count,
        reinterpret_cast<VkImage*>(images)) : VK_ERROR_INITIALIZATION_FAILED;
#else
    (void)self; (void)count; (void)images; return -3;
#endif
}
extern "C" RSF_RUNTIME_API int32_t rsf_fgvk_acquire(rsf_fgvk* self, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
#if defined(RSF_HAVE_FFX_VK)
    return self && index ? self->functions.pOutAcquireNextImageKHR(static_cast<VkDevice>(self->setup.graphics.device), self->chain, timeout,
        reinterpret_cast<VkSemaphore>(static_cast<uintptr_t>(semaphore)), reinterpret_cast<VkFence>(static_cast<uintptr_t>(fence)), index) : VK_ERROR_INITIALIZATION_FAILED;
#else
    (void)self; (void)timeout; (void)semaphore; (void)fence; (void)index; return -3;
#endif
}
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fgvk_enable(rsf_fgvk* self, uint32_t enabled) {
    if (!self || enabled > 1) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_FFX_VK)
    self->enabled = enabled != 0; return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fgvk_prepare(rsf_fgvk* self, void* commands, const rsf_fgvk_frame* frame) try {
    if (!self || !commands || !frame || frame->struct_size < sizeof(*frame)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (frame->abi_version != RSF_FGVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if defined(RSF_HAVE_FFX_VK)
    if (!valid_record(*self, frame)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
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
    if (slot.completion_unknown || (slot.hudless && !slot.submitted_hudless)) return RSF_BACKEND_ERROR_NOT_READY;
    auto device = static_cast<VkDevice>(self->setup.graphics.device);
    // Async interpolation is disabled. A fence queued after SDK Present covers
    // the interpolation callback's HUD-less reads on the graphics queue. SDK
    // presentation uses its own images afterwards. Wait only on busy slot reuse.
    for (auto fence : {slot.source_pending ? slot.source_completion : VK_NULL_HANDLE,
            slot.consumer_pending ? slot.consumer_completion : VK_NULL_HANDLE}) {
        if (fence && self->wait_fences(device, 1, &fence, VK_TRUE, 15000000000ull) != VK_SUCCESS) return RSF_BACKEND_ERROR_NOT_READY;
    }
    VkFenceCreateInfo fence_info{}; fence_info.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO;
    for (auto* fence : {&slot.source_completion, &slot.consumer_completion}) if (!*fence &&
        self->create_fence(device, &fence_info, nullptr, fence) != VK_SUCCESS) return RSF_BACKEND_ERROR_INIT_FAILED;
    slot.source_pending = slot.consumer_pending = false;
    const auto& previous = self->slots[self->latest % self->slots.size()].record;
    const bool reset = self->latest + 1 != record.frame_id || previous.session_id != record.session_id ||
        previous.view_id != record.view_id || previous.resource_generation != record.resource_generation;
    slot.record = record; slot.guides = slot.prepared = slot.hudless = slot.submitted_prepare = slot.submitted_hudless = false;
    if (reset) slot.record.flags |= RSF_FRAME_FLAG_RESET;
    slot.prepare_commands = commands; slot.hudless_commands = nullptr;
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
    if (self->enabled) {
        if (self->dispatch(&self->effect, &data.header) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
        slot.prepared = true; ++self->status.prepared;
    }
    slot.guides = true; self->latest = record.frame_id;
    { std::lock_guard lock(self->capture_guard);
        (void)self->capture.record(static_cast<VkCommandBuffer>(commands), frame->depth, record, "fg-depth", record.flags & RSF_FRAME_FLAG_RESET);
        (void)self->capture.record(static_cast<VkCommandBuffer>(commands), frame->motion, record, "fg-motion-uv", record.flags & RSF_FRAME_FLAG_RESET); }
    return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fgvk_hudless(rsf_fgvk* self, void* commands, const rsf_fgvk_frame* frame) try {
    if (!self || !commands || !frame || frame->struct_size < sizeof(*frame)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (frame->abi_version != RSF_FGVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if defined(RSF_HAVE_FFX_VK)
    if (!valid_record(*self, frame)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!self->enabled && !self->capture.wants(frame->record->frame_id)) return RSF_BACKEND_OK;
    const auto& record = *frame->record; auto& slot = self->slots[record.frame_id % self->slots.size()];
    if (!slot.guides || slot.record.frame_id != record.frame_id || frame->hudless.device != self->setup.graphics.device ||
        !frame->hudless.image || !frame->hudless.copy_source || frame->hudless.format != VK_FORMAT_R8G8B8A8_UNORM ||
        frame->hudless.width != self->width || frame->hudless.height != self->height || frame->hudless.layout != VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!slot.color.image && !allocate_color(*self, slot.color)) return RSF_BACKEND_ERROR_INIT_FAILED;
    auto list = static_cast<VkCommandBuffer>(commands);
    auto input = reinterpret_cast<VkImage>(static_cast<uintptr_t>(frame->hudless.image));
    transition(*self, list, input, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL);
    transition(*self, list, slot.color.image, slot.color.initialized ? VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL : VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
    VkImageCopy region{}; region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1}; region.extent = {self->width, self->height, 1};
    self->copy(list, input, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, slot.color.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
    transition(*self, list, input, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
    transition(*self, list, slot.color.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL); slot.color.initialized = true;
    slot.hudless_commands = commands; slot.hudless = true; if (self->enabled) ++self->status.tagged;
    rsf_srvk_image image{self->setup.graphics.device, reinterpret_cast<uint64_t>(slot.color.image), VK_FORMAT_R8G8B8A8_UNORM,
        self->width, self->height, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
    { std::lock_guard lock(self->capture_guard); (void)self->capture.record(list, image, record, "fg-hudless-color", record.flags & RSF_FRAME_FLAG_RESET); }
    return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" RSF_RUNTIME_API void rsf_fgvk_submitted(rsf_fgvk* self, void* const* commands, uint32_t count) {
#if defined(RSF_HAVE_FFX_VK)
    if (!self || !commands) return;
    for (auto& slot : self->slots) for (uint32_t i = 0; i < count; ++i) {
        if (slot.guides && slot.prepare_commands == commands[i]) slot.submitted_prepare = true;
        if (slot.hudless && slot.hudless_commands == commands[i]) slot.submitted_hudless = true;
    }
    for (auto& slot : self->slots) if (slot.hudless && slot.submitted_prepare && slot.submitted_hudless && !slot.source_pending && !slot.completion_unknown) {
        auto device = static_cast<VkDevice>(self->setup.graphics.device);
        if (self->reset_fences(device, 1, &slot.source_completion) != VK_SUCCESS || self->setup.graphics_submit(self->setup.queue_user,
            0, nullptr, reinterpret_cast<uint64_t>(slot.source_completion)) != VK_SUCCESS) slot.completion_unknown = true;
        else slot.source_pending = true;
    }
#else
    (void)self; (void)commands; (void)count;
#endif
}
extern "C" RSF_RUNTIME_API int32_t rsf_fgvk_present(rsf_fgvk* self, const void* present_info) try {
#if defined(RSF_HAVE_FFX_VK)
    if (!self || !present_info) return VK_ERROR_INITIALIZATION_FAILED;
    const auto& info = *static_cast<const VkPresentInfoKHR*>(present_info);
    if (info.swapchainCount != 1 || !info.pSwapchains || info.pSwapchains[0] != self->chain) return VK_ERROR_INITIALIZATION_FAILED;
    auto& slot = self->slots[self->latest % self->slots.size()];
    const bool valid = self->enabled && self->latest > self->presented && slot.record.frame_id == self->latest &&
        slot.prepared && slot.hudless && slot.submitted_prepare && slot.submitted_hudless && !(slot.record.flags & RSF_FRAME_FLAG_RESET);
    const auto configured = configure_frame(*self, valid ? &slot : nullptr);
    if (configured != RSF_BACKEND_OK) { self->status.result = configured; return VK_ERROR_INITIALIZATION_FAILED; }
    const auto result = self->functions.pOutQueuePresentKHR(static_cast<VkQueue>(self->setup.queues[0]), &info);
    if (valid && (result == VK_SUCCESS || result == VK_SUBOPTIMAL_KHR)) {
        auto device = static_cast<VkDevice>(self->setup.graphics.device);
        if (self->reset_fences(device, 1, &slot.consumer_completion) != VK_SUCCESS || self->setup.graphics_submit(self->setup.queue_user,
            0, nullptr, reinterpret_cast<uint64_t>(slot.consumer_completion)) != VK_SUCCESS) slot.completion_unknown = true;
        else slot.consumer_pending = true;
    }
    self->presented = self->latest; self->status.source_frame = self->latest;
    self->status.enabled = valid ? 1u : 0u; self->status.result = result == VK_SUCCESS || result == VK_SUBOPTIMAL_KHR ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
    if (self->functions.pOutGetLastPresentCountFFXAPI) self->status.last_present_count = self->functions.pOutGetLastPresentCountFFXAPI(self->chain);
    return result;
#else
    (void)self; (void)present_info; return -3;
#endif
} catch (...) { return -3; }
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fgvk_get_status(rsf_fgvk* self, rsf_fgvk_status* status) {
    if (!self || !status || status->struct_size < sizeof(*status)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (status->abi_version != RSF_FGVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if defined(RSF_HAVE_FFX_VK)
    *status = self->status; status->generated_callbacks = self->callbacks.load(); return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
}
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fgvk_drain(rsf_fgvk* self) try {
    if (!self) return RSF_BACKEND_OK;
#if defined(RSF_HAVE_FFX_VK)
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
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fgvk_destroy(rsf_fgvk* self) try {
    if (!self) return RSF_BACKEND_OK;
#if defined(RSF_HAVE_FFX_VK)
    if (self->effect && configure_frame(*self, nullptr) != RSF_BACKEND_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    if (self->chain && rsf_fgvk_drain(self) != RSF_BACKEND_OK) return RSF_BACKEND_ERROR_NOT_READY;
    if (self->effect && self->destroy(&self->effect, nullptr) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self->effect = nullptr;
    if (self->chain) self->functions.pOutDestroySwapchainFFXAPI(static_cast<VkDevice>(self->setup.graphics.device), self->chain, self->allocator, self->swapchain);
    self->chain = VK_NULL_HANDLE;
    if (self->swapchain && self->destroy(&self->swapchain, nullptr) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self->swapchain = nullptr;
#endif
    delete self; return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
