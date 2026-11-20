// SPDX-License-Identifier: MIT
#include <vulkan/vulkan.h>
#include <array>
#include <memory>
#include <new>
#include "native_fg_dx12_vulkan_platform.h"
#include "native_dx12_vulkan_platform.h"
#include "../../backends/common/frame_capture_vk.h"

// The Rust owner only needs these Vulkan result and descriptor constants.
static_assert(VK_NOT_READY == 1 && VK_INCOMPLETE == 5 && VK_ERROR_OUT_OF_HOST_MEMORY == -1 &&
    VK_ERROR_INITIALIZATION_FAILED == -3 && VK_ERROR_FORMAT_NOT_SUPPORTED == -11 && VK_ERROR_UNKNOWN == -13 &&
    VK_ERROR_INVALID_EXTERNAL_HANDLE == -1000072003 && VK_ERROR_OUT_OF_DATE_KHR == -1000001004);
static_assert(VK_FORMAT_R8G8B8A8_UNORM == 37 && VK_FORMAT_B8G8R8A8_UNORM == 44 &&
    VK_FORMAT_R16G16_SFLOAT == 83 && VK_FORMAT_R32_SFLOAT == 100 &&
    VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL == 5 && VK_COLOR_SPACE_SRGB_NONLINEAR_KHR == 0 &&
    VK_SHARING_MODE_EXCLUSIVE == 0);
static_assert(sizeof(rsf_dx12gvk_swapchain) == 56 && sizeof(rsf_dx12gvk_present_info) == 32 &&
    sizeof(rsf_dx12gvk_ops) == 24 * sizeof(void*));

struct rsf_dx12gvk_platform {
    VkDevice device{};
    uint32_t family{};
    uint32_t usage{};
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
int32_t platform_create(const rsf_fgvk_setup* setup, uint32_t usage, rsf_dx12gvk_platform** output) {
    auto self = std::unique_ptr<rsf_dx12gvk_platform>(new (std::nothrow) rsf_dx12gvk_platform{});
    if (!self) return VK_ERROR_OUT_OF_HOST_MEMORY;
    self->device = static_cast<VkDevice>(setup->graphics.device);
    self->family = setup->queue_family;
    self->usage = usage;
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
int32_t create_pool(rsf_dx12gvk_platform* self, uint64_t* output) {
    VkCommandPoolCreateInfo info{VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO};
    info.queueFamilyIndex = self->family;
    info.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
    VkCommandPool pool{};
    const auto result = self->create_pool(self->device, &info, nullptr, &pool);
    if (!result) *output = reinterpret_cast<uint64_t>(pool);
    return result;
}
int32_t create_image(void* user, uint32_t width, uint32_t height, uint32_t format, uint64_t* output) {
    const auto& self = *static_cast<rsf_dx12gvk_platform*>(user);
    VkImageCreateInfo create{VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
    create.imageType = VK_IMAGE_TYPE_2D; create.format = static_cast<VkFormat>(format);
    create.extent = {width, height, 1}; create.mipLevels = create.arrayLayers = 1;
    create.samples = VK_SAMPLE_COUNT_1_BIT; create.tiling = VK_IMAGE_TILING_OPTIMAL;
    create.usage = self.usage | VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_SAMPLED_BIT;
    VkImage target{};
    const auto result = self.create_image(self.device, &create, nullptr, &target);
    if (!result) *output = reinterpret_cast<uint64_t>(target);
    return result;
}
const rsf_vkfg_platform image_operations{
    [](void* user) -> uint32_t {
        const auto& memory = static_cast<rsf_dx12gvk_platform*>(user)->memory;
        uint32_t types = 0;
        for (uint32_t i = 0; i < memory.memoryTypeCount; ++i)
            if (memory.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT) types |= 1u << i;
        return types;
    },
    create_image,
    [](void* user, uint64_t target, uint64_t* size, uint32_t* types) {
        const auto& self = *static_cast<rsf_dx12gvk_platform*>(user);
        VkMemoryRequirements requirements{};
        self.requirements(self.device, image(target), &requirements);
        *size = requirements.size; *types = requirements.memoryTypeBits;
    },
    [](void* user, uint64_t size, uint32_t type, uint64_t* output) -> int32_t {
        const auto& self = *static_cast<rsf_dx12gvk_platform*>(user);
        VkMemoryAllocateInfo allocate{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO};
        allocate.allocationSize = size; allocate.memoryTypeIndex = type;
        VkDeviceMemory memory{};
        const auto result = self.allocate(self.device, &allocate, nullptr, &memory);
        if (!result) *output = reinterpret_cast<uint64_t>(memory);
        return result;
    },
    [](void* user, uint64_t target, uint64_t value) -> int32_t {
        const auto& self = *static_cast<rsf_dx12gvk_platform*>(user);
        return self.bind(self.device, image(target), reinterpret_cast<VkDeviceMemory>(value), 0);
    },
    nullptr, nullptr,
    [](void* user, uint64_t target) {
        const auto& self = *static_cast<rsf_dx12gvk_platform*>(user);
        self.destroy_image(self.device, image(target), nullptr);
    },
    [](void* user, uint64_t value) {
        const auto& self = *static_cast<rsf_dx12gvk_platform*>(user);
        self.free_memory(self.device, reinterpret_cast<VkDeviceMemory>(value), nullptr);
    },
};
int32_t allocate_commands(rsf_dx12gvk_platform* self, uint64_t pool, void** output) {
    VkCommandBufferAllocateInfo info{VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};
    info.commandPool = reinterpret_cast<VkCommandPool>(pool);
    info.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    info.commandBufferCount = 1;
    VkCommandBuffer command{};
    const auto result = self->allocate_commands(self->device, &info, &command);
    if (!result) *output = command;
    return result;
}
void transition(rsf_dx12gvk_platform& self, VkCommandBuffer commands, VkImage target,
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
void copy_input(rsf_dx12gvk_platform& self, VkCommandBuffer commands, const rsf_srvk_image& source,
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

void clear(rsf_dx12gvk_platform* self, void* list, const rsf_dx12vk_shared_image* target) {
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
int32_t acquire_submit(rsf_dx12gvk_platform* self, uint64_t semaphore, uint64_t fence) {
    const auto signal = reinterpret_cast<VkSemaphore>(semaphore);
    VkSubmitInfo submit{VK_STRUCTURE_TYPE_SUBMIT_INFO};
    submit.signalSemaphoreCount = semaphore ? 1 : 0; submit.pSignalSemaphores = &signal;
    return self->submit(self->queue_user, 1, &submit, fence);
}
int32_t present_submit(rsf_dx12gvk_platform* self, void* list, const rsf_dx12gvk_present_info* info,
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
int32_t wait_timeline(rsf_dx12gvk_platform* self, uint64_t semaphore, uint64_t value, uint64_t timeout) {
    const auto timeline = reinterpret_cast<VkSemaphore>(semaphore);
    VkSemaphoreWaitInfo info{VK_STRUCTURE_TYPE_SEMAPHORE_WAIT_INFO};
    info.semaphoreCount = 1; info.pSemaphores = &timeline; info.pValues = &value;
    return self->wait(self->device, &info, timeout);
}
}

const rsf_dx12gvk_ops rsf_dx12gvk_graphics{
    dx12vk_platform::adapter_luid,
    platform_create, [](auto* self) { delete self; },
    create_pool, [](auto* self, uint64_t pool) { self->destroy_pool(self->device, reinterpret_cast<VkCommandPool>(pool), nullptr); },
    &image_operations,
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
    [](auto* self, uint64_t provider, uint32_t backend) { self->capture.set_provider(provider, backend, true); },
    [](auto* self, void* commands, const auto* source, const auto* record, const char* role, uint64_t group) {
        (void)self->capture.record(static_cast<VkCommandBuffer>(commands), *source, *record, role,
            record->flags & RSF_FRAME_FLAG_RESET, true, nullptr, group);
    },
    [](auto* self, uint64_t group) { self->capture.complete_group(group); },
    [](auto* self, uint64_t group) { self->capture.discard_group(group); },
    [](auto* self) -> int32_t { return self->capture.flush() ? VK_SUCCESS : VK_ERROR_UNKNOWN; },
};

rsf_dx12gvk_swapchain rsf_dx12gvk_describe(const void* create_info) {
    const auto& info = *static_cast<const VkSwapchainCreateInfoKHR*>(create_info);
    const auto* extra = static_cast<const VkBaseInStructure*>(info.pNext);
    const bool compatible = !extra || (extra->sType == VK_STRUCTURE_TYPE_SWAPCHAIN_LATENCY_CREATE_INFO_NV && !extra->pNext);
    return {info.imageExtent.width, info.imageExtent.height, static_cast<uint32_t>(info.imageFormat), info.imageUsage,
        static_cast<uint32_t>(info.imageColorSpace), info.imageArrayLayers, static_cast<uint32_t>(info.imageSharingMode),
        info.minImageCount, info.flags, compatible, info.presentMode == VK_PRESENT_MODE_FIFO_KHR ||
            info.presentMode == VK_PRESENT_MODE_FIFO_RELAXED_KHR, reinterpret_cast<uint64_t>(info.oldSwapchain)};
}

int32_t rsf_dx12gvk_present_marshaled(rsf_dx12gvk_present_fn call, void* owner, const void* info, const rsf_frame_record* source, uint32_t explicit_source) {
    rsf_dx12gvk_present_info description{};
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
            const int32_t code = call(owner, &description, source, explicit_source);
            if (vk.pResults && result != VK_INCOMPLETE) vk.pResults[0] = static_cast<VkResult>(result);
            return code;
        }
    }
    return VK_ERROR_INITIALIZATION_FAILED;
}
