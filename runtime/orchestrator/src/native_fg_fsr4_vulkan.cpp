// SPDX-License-Identifier: MIT
#define VK_USE_PLATFORM_WIN32_KHR
#include <windows.h>
#include <vulkan/vulkan.h>
#include <array>
#include <cstring>
#include <memory>
#include <new>
#include "native_fg_fsr4_vulkan_platform.h"
#include "../../backends/common/frame_capture_vk.h"

// The Rust owner only needs these Vulkan result and descriptor constants.
static_assert(VK_NOT_READY == 1 && VK_INCOMPLETE == 5 && VK_ERROR_OUT_OF_HOST_MEMORY == -1 &&
    VK_ERROR_INITIALIZATION_FAILED == -3 && VK_ERROR_FORMAT_NOT_SUPPORTED == -11 && VK_ERROR_UNKNOWN == -13 &&
    VK_ERROR_INVALID_EXTERNAL_HANDLE == -1000072003 && VK_ERROR_OUT_OF_DATE_KHR == -1000001004);
static_assert(VK_FORMAT_R8G8B8A8_UNORM == 37 && VK_FORMAT_B8G8R8A8_UNORM == 44 &&
    VK_FORMAT_R16G16_SFLOAT == 83 && VK_FORMAT_R32_SFLOAT == 100 &&
    VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL == 5 && VK_COLOR_SPACE_SRGB_NONLINEAR_KHR == 0 &&
    VK_SHARING_MODE_EXCLUSIVE == 0);
static_assert(sizeof(rsf_fsr4gvk_swapchain) == 56 && sizeof(rsf_fsr4gvk_present_info) == 32 &&
    sizeof(rsf_fsr4gvk_ops) == 38 * sizeof(void*) && sizeof(rsf_fsr4gvk_options) == 24 &&
    sizeof(rsf_fsr4fg_create) == 48 && sizeof(rsf_fsr4fg_info) == 32 &&
    sizeof(rsf_fsr4fg_frame) == 80 && sizeof(rsf_fsr4fg_present_status) == 32 &&
    sizeof(rsf_dx12vk_shared_image) == 32);

struct rsf_fsr4gvk_platform {
    VkDevice device{};
    uint32_t family{};
    decltype(rsf_fgvk_setup::graphics_submit) submit{};
    decltype(rsf_fgvk_setup::lock_queues) lock{};
    decltype(rsf_fgvk_setup::unlock_queues) unlock{};
    void* queue_user{};
    rsf::FrameCaptureVk capture;
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
    PFN_vkCreateFence create_fence{};
    PFN_vkResetFences reset_fences{};
    PFN_vkWaitForFences wait_fences{};
    PFN_vkDestroyFence destroy_fence{};
};

namespace {
VkImage image(uint64_t value) { return reinterpret_cast<VkImage>(value); }
int32_t directory(const char* path, uint16_t* output, int32_t capacity) {
    return MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path, -1, reinterpret_cast<wchar_t*>(output), capacity);
}
int32_t adapter_luid(const rsf_srvk_setup* setup, uint64_t* luid) {
    const auto get = reinterpret_cast<PFN_vkGetPhysicalDeviceProperties2>(
        setup->instance_proc(setup->instance, "vkGetPhysicalDeviceProperties2"));
    if (!get) return VK_ERROR_EXTENSION_NOT_PRESENT;
    VkPhysicalDeviceIDProperties identity{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES};
    VkPhysicalDeviceProperties2 properties{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2};
    properties.pNext = &identity;
    get(static_cast<VkPhysicalDevice>(setup->physical_device), &properties);
    if (!identity.deviceLUIDValid) return VK_ERROR_INVALID_EXTERNAL_HANDLE;
    std::memcpy(luid, identity.deviceLUID, sizeof(*luid));
    return VK_SUCCESS;
}
int32_t platform_create(const rsf_fgvk_setup* setup, rsf_fsr4gvk_platform** output) {
    auto self = std::unique_ptr<rsf_fsr4gvk_platform>(new (std::nothrow) rsf_fsr4gvk_platform{});
    if (!self) return VK_ERROR_OUT_OF_HOST_MEMORY;
    self->device = static_cast<VkDevice>(setup->graphics.device);
    self->family = setup->queue_family;
    self->submit = setup->graphics_submit;
    self->lock = setup->lock_queues;
    self->unlock = setup->unlock_queues;
    self->queue_user = setup->queue_user;
    auto ip = setup->graphics.instance_proc; auto dp = setup->graphics.device_proc;
#define D(field, name) \
    self->field = reinterpret_cast<PFN_##name>(dp(self->device, #name)); \
    if (!self->field) return VK_ERROR_EXTENSION_NOT_PRESENT
    D(create_image, vkCreateImage); D(destroy_image, vkDestroyImage); D(requirements, vkGetImageMemoryRequirements);
    D(allocate, vkAllocateMemory); D(free_memory, vkFreeMemory); D(bind, vkBindImageMemory);
    D(create_pool, vkCreateCommandPool); D(destroy_pool, vkDestroyCommandPool); D(allocate_commands, vkAllocateCommandBuffers);
    D(reset_commands, vkResetCommandBuffer); D(begin, vkBeginCommandBuffer); D(end, vkEndCommandBuffer);
    D(barrier, vkCmdPipelineBarrier); D(copy, vkCmdCopyImage); D(blit, vkCmdBlitImage); D(clear, vkCmdClearColorImage);
    D(wait, vkWaitSemaphores); D(idle, vkDeviceWaitIdle);
    D(create_fence, vkCreateFence); D(reset_fences, vkResetFences);
    D(wait_fences, vkWaitForFences); D(destroy_fence, vkDestroyFence);
#undef D
    const auto get_memory = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(
        ip(setup->graphics.instance, "vkGetPhysicalDeviceMemoryProperties"));
    self->format_properties = reinterpret_cast<PFN_vkGetPhysicalDeviceFormatProperties>(
        ip(setup->graphics.instance, "vkGetPhysicalDeviceFormatProperties"));
    if (!self->format_properties || !get_memory) return VK_ERROR_EXTENSION_NOT_PRESENT;
    const auto physical = static_cast<VkPhysicalDevice>(setup->graphics.physical_device);
    VkFormatProperties rgba{}, bgra{};
    self->format_properties(physical, VK_FORMAT_R8G8B8A8_UNORM, &rgba);
    self->format_properties(physical, VK_FORMAT_B8G8R8A8_UNORM, &bgra);
    if (!(rgba.optimalTilingFeatures & VK_FORMAT_FEATURE_BLIT_SRC_BIT) || !(bgra.optimalTilingFeatures & VK_FORMAT_FEATURE_BLIT_DST_BIT))
        return VK_ERROR_FORMAT_NOT_SUPPORTED;
    get_memory(physical, &self->memory);
    if (!self->capture.initialize(self->device, dp, self->memory)) return VK_ERROR_INITIALIZATION_FAILED;
    *output = self.release();
    return VK_SUCCESS;
}
int32_t interop_create(const rsf_srvk_setup* setup, rsf_fsr4fg_bridge* bridge, uint64_t luid,
    uint32_t timeline_enabled, rsf_dx12vk_interop** output) {
    const rsf_dx12vk_interop_callbacks callbacks{sizeof(callbacks), 1, luid,
        bridge, RsfFsr4FgCreateImage, RsfFsr4FgReleaseImage, RsfFsr4FgCreateFence};
    return rsf_dx12vk_interop_create(setup, &callbacks, timeline_enabled, output);
}
int32_t create_pool(rsf_fsr4gvk_platform* self, uint64_t* output) {
    VkCommandPoolCreateInfo info{VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO};
    info.queueFamilyIndex = self->family;
    info.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
    VkCommandPool pool{};
    const auto result = self->create_pool(self->device, &info, nullptr, &pool);
    if (!result) *output = reinterpret_cast<uint64_t>(pool);
    return result;
}
int32_t create_image(rsf_fsr4gvk_platform* self, const rsf_fsr4gvk_swapchain* swapchain, uint64_t* output, uint64_t* memory) {
    VkImageCreateInfo create{VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
    create.imageType = VK_IMAGE_TYPE_2D; create.format = static_cast<VkFormat>(swapchain->format);
    create.extent = {swapchain->width, swapchain->height, 1}; create.mipLevels = create.arrayLayers = 1;
    create.samples = VK_SAMPLE_COUNT_1_BIT; create.tiling = VK_IMAGE_TILING_OPTIMAL;
    create.usage = swapchain->usage | VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_SAMPLED_BIT;
    VkImage target{};
    VkResult result = self->create_image(self->device, &create, nullptr, &target);
    if (result) return result;
    *output = reinterpret_cast<uint64_t>(target);
    VkMemoryRequirements requirements{};
    self->requirements(self->device, target, &requirements);
    VkMemoryAllocateInfo allocate{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO}; allocate.allocationSize = requirements.size;
    allocate.memoryTypeIndex = self->memory.memoryTypeCount;
    for (uint32_t i = 0; i < self->memory.memoryTypeCount; ++i)
        if ((requirements.memoryTypeBits & (1u << i)) && (self->memory.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT)) {
            allocate.memoryTypeIndex = i; break;
        }
    if (allocate.memoryTypeIndex == self->memory.memoryTypeCount) return VK_ERROR_OUT_OF_DEVICE_MEMORY;
    VkDeviceMemory allocation{};
    result = self->allocate(self->device, &allocate, nullptr, &allocation);
    if (result) return result;
    *memory = reinterpret_cast<uint64_t>(allocation);
    return self->bind(self->device, target, allocation, 0);
}
int32_t allocate_commands(rsf_fsr4gvk_platform* self, uint64_t pool, void** output) {
    VkCommandBufferAllocateInfo info{VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};
    info.commandPool = reinterpret_cast<VkCommandPool>(pool);
    info.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    info.commandBufferCount = 1;
    VkCommandBuffer command{};
    const auto result = self->allocate_commands(self->device, &info, &command);
    if (!result) *output = command;
    return result;
}
void transition(rsf_fsr4gvk_platform& self, VkCommandBuffer commands, VkImage target,
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
void copy_input(rsf_fsr4gvk_platform& self, VkCommandBuffer commands, const rsf_srvk_image& source,
    const rsf_dx12vk_shared_image& target, bool initialized) {
    const auto family = self.family;
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

void clear(rsf_fsr4gvk_platform* self, void* list, const rsf_dx12vk_shared_image* target) {
    const auto commands = static_cast<VkCommandBuffer>(list);
    const auto img = image(target->image);
    transition(*self, commands, img, VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        self->family, self->family);
    const VkClearColorValue zero{};
    const VkImageSubresourceRange range{VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    self->clear(commands, img, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, &zero, 1, &range);
    transition(*self, commands, img, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_GENERAL,
        self->family, VK_QUEUE_FAMILY_EXTERNAL);
}
int32_t acquire_submit(rsf_fsr4gvk_platform* self, uint64_t semaphore, uint64_t fence) {
    const auto signal = reinterpret_cast<VkSemaphore>(semaphore);
    VkSubmitInfo submit{VK_STRUCTURE_TYPE_SUBMIT_INFO};
    submit.signalSemaphoreCount = semaphore ? 1 : 0; submit.pSignalSemaphores = &signal;
    return self->submit(self->queue_user, 1, &submit, fence);
}
int32_t present_submit(rsf_fsr4gvk_platform* self, void* list, const rsf_fsr4gvk_present_info* info,
    uint64_t semaphore, uint64_t previous, uint64_t ready) {
    std::array<VkSemaphore, 17> waits{};
    std::array<VkPipelineStageFlags, 17> stages{};
    std::array<uint64_t, 17> values{};
    auto count = info->wait_count;
    for (uint32_t i = 0; i < count; ++i) {
        waits[i] = reinterpret_cast<VkSemaphore>(info->waits[i]); stages[i] = VK_PIPELINE_STAGE_ALL_COMMANDS_BIT;
    }
    const auto signal = reinterpret_cast<VkSemaphore>(semaphore);
    if (previous) {
        waits[count] = signal; values[count] = previous;
        stages[count++] = VK_PIPELINE_STAGE_ALL_COMMANDS_BIT;
    }
    VkTimelineSemaphoreSubmitInfo timeline{VK_STRUCTURE_TYPE_TIMELINE_SEMAPHORE_SUBMIT_INFO};
    timeline.waitSemaphoreValueCount = count; timeline.pWaitSemaphoreValues = values.data();
    timeline.signalSemaphoreValueCount = 1; timeline.pSignalSemaphoreValues = &ready;
    const auto commands = static_cast<VkCommandBuffer>(list);
    VkSubmitInfo submit{VK_STRUCTURE_TYPE_SUBMIT_INFO}; submit.pNext = &timeline;
    submit.waitSemaphoreCount = count; submit.pWaitSemaphores = waits.data(); submit.pWaitDstStageMask = stages.data();
    submit.commandBufferCount = 1; submit.pCommandBuffers = &commands;
    submit.signalSemaphoreCount = 1; submit.pSignalSemaphores = &signal;
    return self->submit(self->queue_user, 1, &submit, 0);
}
int32_t wait_timeline(rsf_fsr4gvk_platform* self, uint64_t semaphore, uint64_t value, uint64_t timeout) {
    const auto timeline = reinterpret_cast<VkSemaphore>(semaphore);
    VkSemaphoreWaitInfo info{VK_STRUCTURE_TYPE_SEMAPHORE_WAIT_INFO};
    info.semaphoreCount = 1; info.pSemaphores = &timeline; info.pValues = &value;
    return self->wait(self->device, &info, timeout);
}
constexpr rsf_fsr4gvk_ops operations{
    adapter_luid, directory, RsfFsr4FgProbe, RsfFsr4FgCreate, RsfFsr4FgInfo, RsfFsr4FgStart,
    RsfFsr4FgPresent, RsfFsr4FgDrain, RsfFsr4FgPrepareDestroy, RsfFsr4FgDestroy,
    interop_create, rsf_dx12vk_interop_image, rsf_dx12vk_interop_timeline, rsf_dx12vk_interop_destroy,
    platform_create, [](auto* self) { delete self; },
    create_pool, [](auto* self, uint64_t pool) { self->destroy_pool(self->device, reinterpret_cast<VkCommandPool>(pool), nullptr); },
    create_image, [](auto* self, uint64_t img, uint64_t memory) {
        if (img) self->destroy_image(self->device, image(img), nullptr);
        if (memory) self->free_memory(self->device, reinterpret_cast<VkDeviceMemory>(memory), nullptr);
    },
    allocate_commands,
    [](auto* self, void* list) -> int32_t {
        const auto commands = static_cast<VkCommandBuffer>(list);
        const auto result = self->reset_commands(commands, 0);
        if (result) return result;
        VkCommandBufferBeginInfo info{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};
        info.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
        return self->begin(commands, &info);
    },
    [](auto* self, void* list) -> int32_t { return self->end(static_cast<VkCommandBuffer>(list)); },
    [](auto* self, void* list, const auto* source, const auto* target, uint32_t initialized) {
        copy_input(*self, static_cast<VkCommandBuffer>(list), *source, *target, initialized != 0);
    },
    clear, acquire_submit, present_submit, wait_timeline,
    [](auto* self, uint64_t* output) -> int32_t {
        VkFence fence{};
        const VkFenceCreateInfo info{VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
        const auto result = self->create_fence(self->device, &info, nullptr, &fence);
        if (!result) *output = reinterpret_cast<uint64_t>(fence);
        return result;
    },
    [](auto* self, uint64_t value) -> int32_t {
        const auto fence = reinterpret_cast<VkFence>(value);
        const auto result = self->reset_fences(self->device, 1, &fence);
        return result ? result : self->submit(self->queue_user, 0, nullptr, value);
    },
    [](auto* self, uint64_t value, uint64_t timeout) -> int32_t {
        const auto fence = reinterpret_cast<VkFence>(value);
        return self->wait_fences(self->device, 1, &fence, VK_TRUE, timeout);
    },
    [](auto* self, uint64_t value) { self->destroy_fence(self->device, reinterpret_cast<VkFence>(value), nullptr); },
    [](auto* self) -> int32_t {
        self->lock(self->queue_user);
        const auto result = self->idle(self->device);
        self->unlock(self->queue_user);
        return result;
    },
    [](auto* self, uint64_t provider) { self->capture.set_provider(provider, 4, true); },
    [](auto* self, void* commands, const auto* source, const auto* record, const char* role, uint64_t group) {
        (void)self->capture.record(static_cast<VkCommandBuffer>(commands), *source, *record, role,
            record->flags & RSF_FRAME_FLAG_RESET, true, nullptr, group);
    },
    [](auto* self, uint64_t group) { self->capture.complete_group(group); },
    [](auto* self, uint64_t group) { self->capture.discard_group(group); },
    [](auto* self) -> int32_t { return self->capture.flush() ? VK_SUCCESS : VK_ERROR_UNKNOWN; },
};
}
int32_t rsf_fsr4gvk_probe(const rsf_srvk_setup* setup, rsf_fsr4fg_info* out) {
    return rsf_core_fsr4gvk_probe(&operations, setup, out);
}
int32_t rsf_fsr4gvk_create(const rsf_fgvk_setup* setup, const rsf_fsr4gvk_options* options,
    rsf_fsr4gvk** output, uint64_t* chain) {
    rsf_fsr4gvk_swapchain swapchain{};
    const rsf_fsr4gvk_swapchain* description = nullptr;
    if (setup && setup->struct_size >= sizeof(*setup) && setup->abi_version == RSF_FGVK_ABI_VERSION && setup->create_info &&
        options && options->struct_size >= sizeof(*options) && options->abi_version == 1 && output && chain) {
        const auto& info = *static_cast<const VkSwapchainCreateInfoKHR*>(setup->create_info);
        const auto* extra = static_cast<const VkBaseInStructure*>(info.pNext);
        const bool compatible = !extra || (extra->sType == VK_STRUCTURE_TYPE_SWAPCHAIN_LATENCY_CREATE_INFO_NV && !extra->pNext);
        swapchain = {info.imageExtent.width, info.imageExtent.height, static_cast<uint32_t>(info.imageFormat), info.imageUsage,
            static_cast<uint32_t>(info.imageColorSpace), info.imageArrayLayers, static_cast<uint32_t>(info.imageSharingMode),
            info.minImageCount, info.flags, compatible, info.presentMode == VK_PRESENT_MODE_FIFO_KHR ||
                info.presentMode == VK_PRESENT_MODE_FIFO_RELAXED_KHR, reinterpret_cast<uint64_t>(info.oldSwapchain)};
        description = &swapchain;
    }
    return rsf_core_fsr4gvk_create(&operations, setup, options, description, output, chain);
}
namespace {
int32_t present(rsf_fsr4gvk* owner, const void* info, const rsf_frame_record* source, uint32_t explicit_source) {
    rsf_fsr4gvk_present_info description{};
    std::array<uint64_t, 16> waits{};
    if (info) {
        const auto& vk = *static_cast<const VkPresentInfoKHR*>(info);
        if (!vk.pNext && vk.swapchainCount == 1 && vk.pSwapchains && vk.pImageIndices &&
            vk.waitSemaphoreCount <= waits.size() && (!vk.waitSemaphoreCount || vk.pWaitSemaphores)) {
            for (uint32_t i = 0; i < vk.waitSemaphoreCount; ++i) waits[i] = reinterpret_cast<uint64_t>(vk.pWaitSemaphores[i]);
            description = {reinterpret_cast<uint64_t>(vk.pSwapchains[0]), vk.pImageIndices[0], vk.waitSemaphoreCount,
                waits.data(), nullptr};
            // Marshal pResults separately rather than writing VkResult through an int32_t lvalue.
            int32_t result = VK_INCOMPLETE;
            if (vk.pResults) description.result = &result;
            const int32_t code = rsf_core_fsr4gvk_present(owner, &description, source, explicit_source);
            if (vk.pResults && result != VK_INCOMPLETE) vk.pResults[0] = static_cast<VkResult>(result);
            return code;
        }
    }
    return VK_ERROR_INITIALIZATION_FAILED;
}
}
int32_t rsf_fsr4gvk_present(rsf_fsr4gvk* owner, const void* info) { return present(owner, info, nullptr, 0); }
int32_t rsf_fsr4gvk_present_source(rsf_fsr4gvk* owner, const void* info, const rsf_frame_record* source) {
    return present(owner, info, source, 1);
}

int32_t rsf_fsr4gvk_images(rsf_fsr4gvk* owner, uint32_t* count, uint64_t* output) {
    return rsf_core_fsr4gvk_images(owner, count, output);
}
int32_t rsf_fsr4gvk_acquire(rsf_fsr4gvk* owner, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
    return rsf_core_fsr4gvk_acquire(owner, timeout, semaphore, fence, index);
}
int32_t rsf_fsr4gvk_prepare(rsf_fsr4gvk* owner, void* commands, const rsf_fgvk_frame* frame) {
    return rsf_core_fsr4gvk_prepare(owner, commands, frame);
}
int32_t rsf_fsr4gvk_hudless(rsf_fsr4gvk* owner, void* commands, const rsf_fgvk_frame* frame) {
    return rsf_core_fsr4gvk_hudless(owner, commands, frame);
}
void rsf_fsr4gvk_submitted(rsf_fsr4gvk* owner, void* const* commands, uint32_t count) {
    rsf_core_fsr4gvk_submitted(owner, commands, count);
}
int32_t rsf_fsr4gvk_enable(rsf_fsr4gvk* owner, uint32_t enabled) { return rsf_core_fsr4gvk_enable_options(owner, enabled, 1); }
int32_t rsf_fsr4gvk_enable_options(rsf_fsr4gvk* owner, uint32_t enabled, uint32_t count) {
    return rsf_core_fsr4gvk_enable_options(owner, enabled, count);
}
uint32_t rsf_fsr4gvk_max_generated(const rsf_fsr4gvk* owner) { return rsf_core_fsr4gvk_max_generated(owner); }
int32_t rsf_fsr4gvk_status(rsf_fsr4gvk* owner, rsf_fgvk_status* status) { return rsf_core_fsr4gvk_status(owner, status); }
int32_t rsf_fsr4gvk_drain(rsf_fsr4gvk* owner) { return rsf_core_fsr4gvk_drain(owner); }
int32_t rsf_fsr4gvk_destroy(rsf_fsr4gvk* owner) { return rsf_core_fsr4gvk_destroy(owner); }
