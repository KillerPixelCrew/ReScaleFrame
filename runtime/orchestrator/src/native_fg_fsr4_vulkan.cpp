// SPDX-License-Identifier: GPL-3.0-only
#define VK_USE_PLATFORM_WIN32_KHR
#include <windows.h>
#include <vulkan/vulkan.h>
#include <array>
#include <cmath>
#include <cstring>
#include <new>
#include "native_fg_fsr4_vulkan.h"
#include "../../backends/common/frame_capture_vk.h"
#include <mutex>

struct rsf_fsr4gvk {
    rsf_fgvk_setup setup{};
    rsf_fsr4fg_bridge* bridge{};
    rsf_dx12vk_interop* interop{};
    VkDevice device{};
    VkCommandPool pool{};
    VkSemaphore timeline{};
    uint32_t width{}, height{}, format{}, next_image{};
    uint64_t chain{}, latest{}, last_presented{}, last_done{};
    bool requested{}, active{}, terminal{}, vsync{};
    struct Buffer {
        VkImage image{}; VkDeviceMemory memory{}; VkCommandBuffer commands{};
        rsf_dx12vk_shared_image shared{};
        uint64_t done{}; bool acquired{}, initialized{};
    };
    std::array<Buffer, 3> buffers{};
    struct Slot {
        rsf_frame_record record{};
        rsf_dx12vk_shared_image depth{}, motion{}, hudless{};
        void* prepare_commands{}; void* hudless_commands{};
        uint64_t done{};
        float motion_scale_x{}, motion_scale_y{};
        bool prepared{}, hudless_recorded{}, prepare_submitted{}, hudless_submitted{};
        bool guides_initialized{}, hudless_initialized{};
    };
    std::array<Slot, 6> slots{};
    rsf_dx12vk_shared_image fallback_depth{}, fallback_motion{};
    bool fallback_initialized{};
    rsf_fgvk_status status{sizeof(status), RSF_FGVK_ABI_VERSION};
    rsf::FrameCaptureVk capture;
    std::mutex capture_guard;
    VkPhysicalDeviceMemoryProperties memory{};
    PFN_vkGetPhysicalDeviceFormatProperties format_properties{};
    PFN_vkCreateImage create_image{};
    PFN_vkDestroyImage destroy_image{};
    PFN_vkGetImageMemoryRequirements requirements{};
    PFN_vkAllocateMemory allocate{};
    PFN_vkFreeMemory free_memory{};
    PFN_vkBindImageMemory bind{};
    PFN_vkCreateCommandPool create_pool{};
    PFN_vkDestroyCommandPool destroy_pool{};
    PFN_vkAllocateCommandBuffers allocate_commands{};
    PFN_vkResetCommandBuffer reset_commands{};
    PFN_vkBeginCommandBuffer begin{};
    PFN_vkEndCommandBuffer end{};
    PFN_vkCmdPipelineBarrier barrier{};
    PFN_vkCmdCopyImage copy{};
    PFN_vkCmdBlitImage blit{};
    PFN_vkCmdClearColorImage clear{};
    PFN_vkWaitSemaphores wait{};
    PFN_vkDeviceWaitIdle idle{};
};

namespace {
bool runtime_directory(const char* path, wchar_t* output, int capacity) {
    return path && MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path, -1, output, capacity) > 1;
}
VkImage image(uint64_t value) { return reinterpret_cast<VkImage>(value); }
int wait_for(rsf_fsr4gvk& self, uint64_t value, uint64_t timeout = 10000000000ull) {
    if (!value) return VK_SUCCESS;
    VkSemaphoreWaitInfo info{VK_STRUCTURE_TYPE_SEMAPHORE_WAIT_INFO};
    info.semaphoreCount = 1; info.pSemaphores = &self.timeline; info.pValues = &value;
    return self.wait(self.device, &info, timeout);
}
void transition(rsf_fsr4gvk& self, VkCommandBuffer commands, VkImage target,
    VkImageLayout before, VkImageLayout after, uint32_t source_family, uint32_t target_family) {
    VkImageMemoryBarrier barrier{VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER};
    barrier.srcAccessMask = before == VK_IMAGE_LAYOUT_UNDEFINED || source_family == VK_QUEUE_FAMILY_EXTERNAL ? 0 :
        VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    barrier.dstAccessMask = target_family == VK_QUEUE_FAMILY_EXTERNAL ? 0 :
        VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    barrier.oldLayout = before; barrier.newLayout = after;
    barrier.srcQueueFamilyIndex = source_family; barrier.dstQueueFamilyIndex = target_family;
    barrier.image = target; barrier.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    self.barrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT,
        0, 0, nullptr, 0, nullptr, 1, &barrier);
}
void copy_input(rsf_fsr4gvk& self, VkCommandBuffer commands, const rsf_srvk_image& source,
    const rsf_dx12vk_shared_image& target, bool initialized) {
    const auto family = self.setup.queue_family;
    transition(self, commands, image(source.image), static_cast<VkImageLayout>(source.layout),
        VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_QUEUE_FAMILY_IGNORED, VK_QUEUE_FAMILY_IGNORED);
    transition(self, commands, image(target.image), initialized ? VK_IMAGE_LAYOUT_GENERAL : VK_IMAGE_LAYOUT_UNDEFINED,
        VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, initialized ? VK_QUEUE_FAMILY_EXTERNAL : family, family);
    if (source.format == target.format) {
        VkImageCopy region{};
        region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
        region.extent = {target.width, target.height, 1};
        self.copy(commands, image(source.image), VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
            image(target.image), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
    } else {
        VkImageBlit region{};
        region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
        region.srcOffsets[1] = {static_cast<int32_t>(source.width), static_cast<int32_t>(source.height), 1};
        region.dstOffsets[1] = {static_cast<int32_t>(target.width), static_cast<int32_t>(target.height), 1};
        self.blit(commands, image(source.image), VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
            image(target.image), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region, VK_FILTER_NEAREST);
    }
    transition(self, commands, image(source.image), VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        static_cast<VkImageLayout>(source.layout), VK_QUEUE_FAMILY_IGNORED, VK_QUEUE_FAMILY_IGNORED);
    transition(self, commands, image(target.image), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        VK_IMAGE_LAYOUT_GENERAL, family, VK_QUEUE_FAMILY_EXTERNAL);
}
bool valid_image(const rsf_fsr4gvk& self, const rsf_srvk_image& input, uint32_t format, uint32_t width, uint32_t height) {
    return input.device == self.setup.graphics.device && input.image && input.copy_source && input.format == format &&
        input.width == width && input.height == height && input.layout == VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
}
bool valid_frame(const rsf_fsr4gvk& self, const rsf_fgvk_frame* frame) {
    if (!frame || frame->struct_size < sizeof(*frame) || frame->abi_version != RSF_FGVK_ABI_VERSION || !frame->record) return false;
    const auto& record = *frame->record;
    if (record.struct_size < sizeof(record) || record.abi_version != RSF_GAME_FRAME_ABI_VERSION ||
        !record.frame_id || !record.session_id || !record.render_width || !record.render_height ||
        record.output_width != self.width || record.output_height != self.height ||
        record.camera.struct_size < sizeof(record.camera) || record.camera.abi_version != RSF_GAME_FRAME_ABI_VERSION ||
        record.camera.motion_jittered || record.camera.depth_inverted != self.setup.graphics.depth_inverted ||
        !std::isfinite(frame->motion_scale_x) || !std::isfinite(frame->motion_scale_y) ||
        !frame->motion_scale_x || !frame->motion_scale_y) return false;
    for (uint32_t i = 0; i < 16; ++i)
        if (!std::isfinite(record.camera.world_to_view[i]) || !std::isfinite(record.camera.view_to_clip[i])) return false;
    return std::isfinite(record.camera.jitter_pixels[0]) && std::isfinite(record.camera.jitter_pixels[1]);
}
int create_buffer(rsf_fsr4gvk& self, rsf_fsr4gvk::Buffer& buffer) {
    VkImageCreateInfo create{VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
    create.imageType = VK_IMAGE_TYPE_2D; create.format = static_cast<VkFormat>(self.format);
    create.extent = {self.width, self.height, 1}; create.mipLevels = create.arrayLayers = 1;
    create.samples = VK_SAMPLE_COUNT_1_BIT; create.tiling = VK_IMAGE_TILING_OPTIMAL;
    create.usage = static_cast<const VkSwapchainCreateInfoKHR*>(self.setup.create_info)->imageUsage |
        VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_SAMPLED_BIT;
    VkResult result = self.create_image(self.device, &create, nullptr, &buffer.image);
    if (result) return result;
    VkMemoryRequirements requirements{};
    self.requirements(self.device, buffer.image, &requirements);
    VkMemoryAllocateInfo allocate{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO}; allocate.allocationSize = requirements.size;
    allocate.memoryTypeIndex = self.memory.memoryTypeCount;
    for (uint32_t i = 0; i < self.memory.memoryTypeCount; ++i)
        if ((requirements.memoryTypeBits & (1u << i)) && (self.memory.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT)) {
            allocate.memoryTypeIndex = i; break;
        }
    if (allocate.memoryTypeIndex == self.memory.memoryTypeCount) return VK_ERROR_OUT_OF_DEVICE_MEMORY;
    result = self.allocate(self.device, &allocate, nullptr, &buffer.memory);
    if (!result) result = self.bind(self.device, buffer.image, buffer.memory, 0);
    if (result) return result;
    return rsf_dx12vk_interop_image(self.interop, VK_FORMAT_B8G8R8A8_UNORM, self.width, self.height, &buffer.shared);
}
}

int32_t rsf_fsr4gvk_probe(const rsf_srvk_setup* setup, rsf_fsr4fg_info* info) {
    if (!setup || setup->struct_size < sizeof(*setup) || !info || !setup->instance ||
        !setup->physical_device || !setup->instance_proc || !setup->runtime_directory_utf8) return RSF_FSR4FG_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_SRVK_ABI_VERSION) return RSF_FSR4FG_ABI_MISMATCH;
    wchar_t directory[32768]{};
    if (!runtime_directory(setup->runtime_directory_utf8, directory, 32768)) return RSF_FSR4FG_INVALID_ARGUMENT;
    const auto get = reinterpret_cast<PFN_vkGetPhysicalDeviceProperties2>(setup->instance_proc(setup->instance, "vkGetPhysicalDeviceProperties2"));
    if (!get) return VK_ERROR_EXTENSION_NOT_PRESENT;
    VkPhysicalDeviceIDProperties identity{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES};
    VkPhysicalDeviceProperties2 properties{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2}; properties.pNext = &identity;
    get(static_cast<VkPhysicalDevice>(setup->physical_device), &properties);
    if (!identity.deviceLUIDValid) return VK_ERROR_INVALID_EXTERNAL_HANDLE;
    uint64_t luid = 0; std::memcpy(&luid, identity.deviceLUID, sizeof(luid));
    return RsfFsr4FgProbe(luid, reinterpret_cast<const uint16_t*>(directory), info);
}

int32_t rsf_fsr4gvk_create(const rsf_fgvk_setup* setup, const rsf_fsr4gvk_options* options,
    rsf_fsr4gvk** output, uint64_t* chain) {
    if (!output || !chain) return RSF_FSR4FG_INVALID_ARGUMENT;
    *output = nullptr; *chain = 0;
    if (!setup || setup->struct_size < sizeof(*setup) || !options || options->struct_size < sizeof(*options)) return RSF_FSR4FG_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_FGVK_ABI_VERSION || options->abi_version != 1) return RSF_FSR4FG_ABI_MISMATCH;
    if (!setup->create_info || !setup->graphics.instance_proc || !setup->graphics.device_proc || !setup->graphics.device ||
        !setup->graphics.physical_device || !setup->graphics_submit || !setup->lock_queues || !setup->unlock_queues ||
        !options->window || options->timeline_enabled != 1 || !options->generated_count ||
        !setup->graphics.runtime_directory_utf8 || setup->graphics.motion_jittered || options->generated_count != 1) return RSF_FSR4FG_INVALID_ARGUMENT;
    const auto& info = *static_cast<const VkSwapchainCreateInfoKHR*>(setup->create_info);
    const auto* extra = static_cast<const VkBaseInStructure*>(info.pNext);
    const bool known_latency = extra && extra->sType == VK_STRUCTURE_TYPE_SWAPCHAIN_LATENCY_CREATE_INFO_NV && !extra->pNext;
    if (info.oldSwapchain || info.imageArrayLayers != 1 || info.imageSharingMode != VK_SHARING_MODE_EXCLUSIVE ||
        !info.imageExtent.width || !info.imageExtent.height || info.imageColorSpace != VK_COLOR_SPACE_SRGB_NONLINEAR_KHR ||
        (info.imageFormat != VK_FORMAT_R8G8B8A8_UNORM && info.imageFormat != VK_FORMAT_B8G8R8A8_UNORM) ||
        (info.pNext && !known_latency) || info.flags || info.minImageCount > 3) return VK_ERROR_FORMAT_NOT_SUPPORTED;
    wchar_t directory[32768]{};
    if (!runtime_directory(setup->graphics.runtime_directory_utf8, directory, 32768)) return RSF_FSR4FG_INVALID_ARGUMENT;
    auto* self = new (std::nothrow) rsf_fsr4gvk{};
    if (!self) return VK_ERROR_OUT_OF_HOST_MEMORY;
    self->setup = *setup; self->device = static_cast<VkDevice>(setup->graphics.device);
    self->width = info.imageExtent.width; self->height = info.imageExtent.height; self->format = info.imageFormat;
    self->vsync = info.presentMode == VK_PRESENT_MODE_FIFO_KHR || info.presentMode == VK_PRESENT_MODE_FIFO_RELAXED_KHR;
    auto ip = setup->graphics.instance_proc; auto dp = setup->graphics.device_proc;
#define I(field, name) self->field = reinterpret_cast<PFN_##name>(ip(setup->graphics.instance, #name))
#define D(field, name) self->field = reinterpret_cast<PFN_##name>(dp(setup->graphics.device, #name))
    I(format_properties, vkGetPhysicalDeviceFormatProperties);
    D(create_image, vkCreateImage); D(destroy_image, vkDestroyImage); D(requirements, vkGetImageMemoryRequirements);
    D(allocate, vkAllocateMemory); D(free_memory, vkFreeMemory); D(bind, vkBindImageMemory);
    D(create_pool, vkCreateCommandPool); D(destroy_pool, vkDestroyCommandPool); D(allocate_commands, vkAllocateCommandBuffers);
    D(reset_commands, vkResetCommandBuffer); D(begin, vkBeginCommandBuffer); D(end, vkEndCommandBuffer);
    D(barrier, vkCmdPipelineBarrier); D(copy, vkCmdCopyImage); D(blit, vkCmdBlitImage); D(clear, vkCmdClearColorImage);
    D(wait, vkWaitSemaphores); D(idle, vkDeviceWaitIdle);
#undef I
#undef D
    auto getMemory = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(ip(setup->graphics.instance, "vkGetPhysicalDeviceMemoryProperties"));
    auto getProperties = reinterpret_cast<PFN_vkGetPhysicalDeviceProperties2>(ip(setup->graphics.instance, "vkGetPhysicalDeviceProperties2"));
    if (!self->format_properties || !self->create_image || !self->destroy_image || !self->requirements || !self->allocate ||
        !self->free_memory || !self->bind || !self->create_pool || !self->destroy_pool || !self->allocate_commands ||
        !self->reset_commands || !self->begin || !self->end || !self->barrier || !self->copy || !self->blit ||
        !self->clear || !self->wait || !self->idle || !getMemory || !getProperties) { delete self; return VK_ERROR_EXTENSION_NOT_PRESENT; }
    auto physical = static_cast<VkPhysicalDevice>(setup->graphics.physical_device);
    VkFormatProperties rgba{}, bgra{};
    self->format_properties(physical, VK_FORMAT_R8G8B8A8_UNORM, &rgba);
    self->format_properties(physical, VK_FORMAT_B8G8R8A8_UNORM, &bgra);
    if (!(rgba.optimalTilingFeatures & VK_FORMAT_FEATURE_BLIT_SRC_BIT) || !(bgra.optimalTilingFeatures & VK_FORMAT_FEATURE_BLIT_DST_BIT)) {
        delete self; return VK_ERROR_FORMAT_NOT_SUPPORTED;
    }
    getMemory(physical, &self->memory);
    self->capture.set_provider(0, 4);
    if (!self->capture.initialize(self->device, setup->graphics.device_proc, self->memory)) {
        delete self; return VK_ERROR_INITIALIZATION_FAILED;
    }
    VkPhysicalDeviceIDProperties identity{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES};
    VkPhysicalDeviceProperties2 properties{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2}; properties.pNext = &identity;
    getProperties(physical, &properties);
    uint64_t luid = 0; std::memcpy(&luid, identity.deviceLUID, sizeof(luid));
    rsf_fsr4fg_create create{sizeof(create), 1, luid, reinterpret_cast<const uint16_t*>(directory),
        self->width, self->height, setup->graphics.depth_inverted, setup->graphics.depth_infinite,
        setup->graphics.motion_jittered, options->generated_count};
    int result = identity.deviceLUIDValid ? RsfFsr4FgCreate(&create, &self->bridge) : VK_ERROR_INVALID_EXTERNAL_HANDLE;
    rsf_fsr4fg_info provider{sizeof(provider), 1};
    if (!result) result = RsfFsr4FgInfo(self->bridge, &provider);
    if (!result) {
        const rsf_dx12vk_interop_callbacks callbacks{sizeof(callbacks), 1, provider.adapter_luid,
            self->bridge, RsfFsr4FgCreateImage, RsfFsr4FgReleaseImage, RsfFsr4FgCreateFence};
        result = rsf_dx12vk_interop_create(&setup->graphics, &callbacks, options->timeline_enabled, &self->interop);
        self->capture.set_provider(provider.provider_version, 4);
    }
    if (!result) {
        self->timeline = reinterpret_cast<VkSemaphore>(rsf_dx12vk_interop_timeline(self->interop));
        VkCommandPoolCreateInfo pool{VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO};
        pool.queueFamilyIndex = setup->queue_family; pool.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
        result = self->create_pool(self->device, &pool, nullptr, &self->pool);
    }
    for (auto& buffer : self->buffers) {
        if (result) break;
        result = create_buffer(*self, buffer);
        if (!result) {
            VkCommandBufferAllocateInfo allocate{VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};
            allocate.commandPool = self->pool; allocate.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY; allocate.commandBufferCount = 1;
            result = self->allocate_commands(self->device, &allocate, &buffer.commands);
        }
    }
    if (!result) result = rsf_dx12vk_interop_image(self->interop, VK_FORMAT_R32_SFLOAT, 1, 1, &self->fallback_depth);
    if (!result) result = rsf_dx12vk_interop_image(self->interop, VK_FORMAT_R16G16_SFLOAT, 1, 1, &self->fallback_motion);
    if (!result) result = RsfFsr4FgStart(self->bridge, options->window);
    // create_info was borrowed for creation only; never retain its pointer.
    self->setup.create_info = nullptr;
    if (result) {
        if (rsf_fsr4gvk_destroy(self)) { *output = self; }
        return result;
    }
    self->chain = reinterpret_cast<uint64_t>(self);
    self->status.available = 1;
    *output = self; *chain = self->chain;
    return 0;
}

int32_t rsf_fsr4gvk_images(rsf_fsr4gvk* self, uint32_t* count, uint64_t* output) {
    if (!self || !count) return VK_ERROR_INITIALIZATION_FAILED;
    const uint32_t capacity = *count;
    *count = static_cast<uint32_t>(self->buffers.size());
    if (!output) return VK_SUCCESS;
    const uint32_t copied = capacity < *count ? capacity : *count;
    for (uint32_t i = 0; i < copied; ++i) output[i] = reinterpret_cast<uint64_t>(self->buffers[i].image);
    *count = copied;
    return copied == self->buffers.size() ? VK_SUCCESS : VK_INCOMPLETE;
}

int32_t rsf_fsr4gvk_acquire(rsf_fsr4gvk* self, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
    if (!self || !index || (!semaphore && !fence) || self->terminal) return VK_ERROR_INITIALIZATION_FAILED;
    auto& buffer = self->buffers[self->next_image];
    if (buffer.acquired) return VK_NOT_READY;
    int result = wait_for(*self, buffer.done, timeout);
    if (result) return result;
    VkSemaphore signal = reinterpret_cast<VkSemaphore>(semaphore);
    VkSubmitInfo submit{VK_STRUCTURE_TYPE_SUBMIT_INFO};
    submit.signalSemaphoreCount = semaphore ? 1 : 0; submit.pSignalSemaphores = &signal;
    result = self->setup.graphics_submit(self->setup.queue_user, 1, &submit, fence);
    if (result) return result;
    buffer.acquired = true; *index = self->next_image;
    self->next_image = (self->next_image + 1) % static_cast<uint32_t>(self->buffers.size());
    return VK_SUCCESS;
}

int32_t rsf_fsr4gvk_prepare(rsf_fsr4gvk* self, void* commands, const rsf_fgvk_frame* frame) {
    if (!self || !commands || self->terminal || !valid_frame(*self, frame)) return RSF_FSR4FG_INVALID_ARGUMENT;
    const auto& record = *frame->record;
    if (!valid_image(*self, frame->depth, VK_FORMAT_R32_SFLOAT, record.render_width, record.render_height) ||
        !valid_image(*self, frame->motion, VK_FORMAT_R16G16_SFLOAT, record.render_width, record.render_height)) return RSF_FSR4FG_INVALID_ARGUMENT;
    auto& slot = self->slots[record.frame_id % self->slots.size()];
    if ((slot.prepared && !slot.prepare_submitted) || (slot.hudless_recorded && !slot.hudless_submitted)) return VK_NOT_READY;
    int result = wait_for(*self, slot.done);
    if (result) return result;
    if (slot.depth.image && (slot.depth.width != record.render_width || slot.depth.height != record.render_height)) return VK_ERROR_OUT_OF_DATE_KHR;
    if (!slot.depth.image) result = rsf_dx12vk_interop_image(self->interop, VK_FORMAT_R32_SFLOAT, record.render_width, record.render_height, &slot.depth);
    if (!result && !slot.motion.image) result = rsf_dx12vk_interop_image(self->interop, VK_FORMAT_R16G16_SFLOAT, record.render_width, record.render_height, &slot.motion);
    if (!result && !slot.hudless.image) result = rsf_dx12vk_interop_image(self->interop, VK_FORMAT_B8G8R8A8_UNORM, self->width, self->height, &slot.hudless);
    if (result) return result;
    slot.record = record; slot.prepare_commands = commands; slot.hudless_commands = nullptr;
    slot.motion_scale_x = frame->motion_scale_x; slot.motion_scale_y = frame->motion_scale_y;
    slot.prepared = true; slot.hudless_recorded = slot.prepare_submitted = slot.hudless_submitted = false;
    copy_input(*self, static_cast<VkCommandBuffer>(commands), frame->depth, slot.depth, slot.guides_initialized);
    copy_input(*self, static_cast<VkCommandBuffer>(commands), frame->motion, slot.motion, slot.guides_initialized);
    {
        std::lock_guard lock(self->capture_guard);
        (void)self->capture.record(static_cast<VkCommandBuffer>(commands), frame->depth, record, "fg-depth", record.flags & RSF_FRAME_FLAG_RESET);
        (void)self->capture.record(static_cast<VkCommandBuffer>(commands), frame->motion, record, "fg-motion-uv", record.flags & RSF_FRAME_FLAG_RESET);
    }
    self->latest = record.frame_id; ++self->status.prepared;
    return 0;
}

int32_t rsf_fsr4gvk_hudless(rsf_fsr4gvk* self, void* commands, const rsf_fgvk_frame* frame) {
    if (!self || !commands || self->terminal || !valid_frame(*self, frame)) return RSF_FSR4FG_INVALID_ARGUMENT;
    auto& slot = self->slots[frame->record->frame_id % self->slots.size()];
    if (!slot.prepared || slot.record.frame_id != frame->record->frame_id || slot.hudless_recorded ||
        !valid_image(*self, frame->hudless, VK_FORMAT_R8G8B8A8_UNORM, self->width, self->height)) return RSF_FSR4FG_INVALID_ARGUMENT;
    copy_input(*self, static_cast<VkCommandBuffer>(commands), frame->hudless, slot.hudless, slot.hudless_initialized);
    {
        std::lock_guard lock(self->capture_guard);
        (void)self->capture.record(static_cast<VkCommandBuffer>(commands), frame->hudless, *frame->record,
            "fg-hudless-color", frame->record->flags & RSF_FRAME_FLAG_RESET);
    }
    slot.hudless_recorded = true; slot.hudless_commands = commands; ++self->status.tagged;
    return 0;
}
void rsf_fsr4gvk_submitted(rsf_fsr4gvk* self, void* const* commands, uint32_t count) {
    if (!self || !commands) return;
    for (auto& slot : self->slots) for (uint32_t i = 0; i < count; ++i) {
        if (slot.prepared && slot.prepare_commands == commands[i]) slot.prepare_submitted = true;
        if (slot.hudless_recorded && slot.hudless_commands == commands[i]) slot.hudless_submitted = true;
        if (slot.prepare_submitted) slot.guides_initialized = true;
        if (slot.hudless_submitted) slot.hudless_initialized = true;
    }
}

int32_t rsf_fsr4gvk_enable(rsf_fsr4gvk* self, uint32_t enabled) {
    if (!self || enabled > 1 || self->terminal) return RSF_FSR4FG_INVALID_ARGUMENT;
    self->requested = enabled != 0;
    return 0;
}
int32_t rsf_fsr4gvk_enable_options(rsf_fsr4gvk* self, uint32_t enabled, uint32_t count) {
    if (!self || enabled > 1 || self->terminal) return RSF_FSR4FG_INVALID_ARGUMENT;
    if (count != 1) return RSF_FSR4FG_NOT_SUPPORTED;
    self->requested = enabled != 0;
    return 0;
}
uint32_t rsf_fsr4gvk_max_generated(const rsf_fsr4gvk* self) { return self ? 1u : 0u; }

int32_t rsf_fsr4gvk_present(rsf_fsr4gvk* self, const void* present_info) {
    if (!self || !present_info || self->terminal) return VK_ERROR_INITIALIZATION_FAILED;
    const auto& info = *static_cast<const VkPresentInfoKHR*>(present_info);
    if (info.pNext || info.swapchainCount != 1 || !info.pSwapchains || info.pSwapchains[0] != reinterpret_cast<VkSwapchainKHR>(self->chain) ||
        !info.pImageIndices || info.pImageIndices[0] >= self->buffers.size() || info.waitSemaphoreCount > 16) return VK_ERROR_INITIALIZATION_FAILED;
    auto& buffer = self->buffers[info.pImageIndices[0]];
    if (!buffer.acquired) return VK_ERROR_INITIALIZATION_FAILED;
    auto& slot = self->slots[self->latest % self->slots.size()];
    const bool valid = self->requested && self->latest > self->last_presented && slot.record.frame_id == self->latest &&
        slot.prepared && slot.prepare_submitted && slot.hudless_recorded && slot.hudless_submitted &&
        !(slot.record.flags & (RSF_FRAME_FLAG_NO_FG | RSF_FRAME_FLAG_AMBIGUOUS_ID));
    self->active = valid;
    int result = self->reset_commands(buffer.commands, 0);
    VkCommandBufferBeginInfo begin{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO}; begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    if (!result) result = self->begin(buffer.commands, &begin);
    if (result) return result;
    if (!self->fallback_initialized) {
        for (const auto* target : {&self->fallback_depth, &self->fallback_motion}) {
            transition(*self, buffer.commands, image(target->image), VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                self->setup.queue_family, self->setup.queue_family);
            VkClearColorValue zero{};
            VkImageSubresourceRange range{VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
            self->clear(buffer.commands, image(target->image), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, &zero, 1, &range);
            transition(*self, buffer.commands, image(target->image), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_GENERAL,
                self->setup.queue_family, VK_QUEUE_FAMILY_EXTERNAL);
        }
    }
    rsf_srvk_image source{self->setup.graphics.device, reinterpret_cast<uint64_t>(buffer.image), self->format,
        self->width, self->height, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
    copy_input(*self, buffer.commands, source, buffer.shared, buffer.initialized);
    result = self->end(buffer.commands);
    if (result) return result;
    std::array<VkSemaphore, 17> waits{};
    std::array<VkPipelineStageFlags, 17> stages{};
    std::array<uint64_t, 17> values{};
    uint32_t wait_count = info.waitSemaphoreCount;
    for (uint32_t i = 0; i < wait_count; ++i) { waits[i] = info.pWaitSemaphores[i]; stages[i] = VK_PIPELINE_STAGE_ALL_COMMANDS_BIT; }
    // A shared bidirectional fence is one ordered timeline, even across slots.
    // Never let Vulkan ready(n+1) overtake DX12 done(n).
    if (self->last_done) { waits[wait_count] = self->timeline; values[wait_count] = self->last_done;
        stages[wait_count++] = VK_PIPELINE_STAGE_ALL_COMMANDS_BIT; }
    const uint64_t ready = self->last_done + 1, done = ready + 1;
    VkTimelineSemaphoreSubmitInfo timeline{VK_STRUCTURE_TYPE_TIMELINE_SEMAPHORE_SUBMIT_INFO};
    timeline.waitSemaphoreValueCount = wait_count; timeline.pWaitSemaphoreValues = values.data();
    timeline.signalSemaphoreValueCount = 1; timeline.pSignalSemaphoreValues = &ready;
    VkSubmitInfo submit{VK_STRUCTURE_TYPE_SUBMIT_INFO}; submit.pNext = &timeline;
    submit.waitSemaphoreCount = wait_count; submit.pWaitSemaphores = waits.data(); submit.pWaitDstStageMask = stages.data();
    submit.commandBufferCount = 1; submit.pCommandBuffers = &buffer.commands;
    submit.signalSemaphoreCount = 1; submit.pSignalSemaphores = &self->timeline;
    result = self->setup.graphics_submit(self->setup.queue_user, 1, &submit, 0);
    if (result) { self->terminal = true; return result; }
    self->fallback_initialized = buffer.initialized = true;
    buffer.done = done; self->last_done = done;
    if (valid) slot.done = done;
    rsf_fsr4fg_frame frame{}; frame.struct_size = sizeof(frame); frame.abi_version = 1;
    frame.record = slot.record.frame_id == self->latest ? &slot.record : nullptr;
    frame.enabled = valid ? 1u : 0u;
    frame.vsync = self->vsync ? 1u : 0u;
    frame.color = buffer.shared.dx12_resource;
    frame.depth = valid ? slot.depth.dx12_resource : self->fallback_depth.dx12_resource;
    frame.motion = valid ? slot.motion.dx12_resource : self->fallback_motion.dx12_resource;
    frame.hudless = valid ? slot.hudless.dx12_resource : buffer.shared.dx12_resource;
    frame.motion_scale_x = valid ? slot.motion_scale_x : 1.0f;
    frame.motion_scale_y = valid ? slot.motion_scale_y : 1.0f;
    frame.ready_value = ready; frame.done_value = done;
    rsf_fsr4fg_present_status status{sizeof(status), 1};
    result = RsfFsr4FgPresent(self->bridge, &frame, &status);
    if (status.terminal) self->terminal = true;
    buffer.acquired = false;
    self->last_presented = self->latest;
    self->status.source_frame = self->latest; self->status.last_present_count = status.frames_presented;
    self->status.enabled = status.fg_enabled; self->status.result = result ? result : status.result;
    // This counter comes only from observed genuine SDK output-resource callbacks
    // when pinned diagnostic capture is enabled, never from SDK present counts.
    self->status.generated_callbacks = status.generated_callbacks;
    if (info.pResults) info.pResults[0] = result ? VK_ERROR_UNKNOWN : VK_SUCCESS;
    return result ? VK_ERROR_UNKNOWN : VK_SUCCESS;
}

int32_t rsf_fsr4gvk_status(rsf_fsr4gvk* self, rsf_fgvk_status* status) {
    if (!self || !status || status->struct_size < sizeof(*status)) return RSF_FSR4FG_INVALID_ARGUMENT;
    if (status->abi_version != RSF_FGVK_ABI_VERSION) return RSF_FSR4FG_ABI_MISMATCH;
    *status = self->status; return 0;
}
int32_t rsf_fsr4gvk_drain(rsf_fsr4gvk* self) {
    if (!self) return 0;
    if (self->terminal) return VK_NOT_READY;
    int result = wait_for(*self, self->last_done);
    if (result) return result;
    self->setup.lock_queues(self->setup.queue_user);
    result = self->idle(self->device);
    self->setup.unlock_queues(self->setup.queue_user);
    if (!result && self->bridge) result = RsfFsr4FgDrain(self->bridge);
    if (!result) {
        std::lock_guard lock(self->capture_guard);
        if (!self->capture.flush()) result = VK_ERROR_UNKNOWN;
    }
    return result;
}
int32_t rsf_fsr4gvk_destroy(rsf_fsr4gvk* self) {
    if (!self) return 0;
    int result = rsf_fsr4gvk_drain(self);
    if (!result && self->bridge) result = RsfFsr4FgPrepareDestroy(self->bridge);
    if (result) return result;
    if (self->pool) self->destroy_pool(self->device, self->pool, nullptr);
    for (auto& buffer : self->buffers) {
        if (buffer.image) self->destroy_image(self->device, buffer.image, nullptr);
        if (buffer.memory) self->free_memory(self->device, buffer.memory, nullptr);
    }
    result = rsf_dx12vk_interop_destroy(self->interop, 1);
    if (result) return result;
    self->interop = nullptr;
    result = RsfFsr4FgDestroy(self->bridge);
    if (result) return result;
    delete self;
    return 0;
}
