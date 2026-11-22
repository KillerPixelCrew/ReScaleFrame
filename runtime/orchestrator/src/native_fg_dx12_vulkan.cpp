// SPDX-License-Identifier: MIT
#include <vulkan/vulkan.h>
#include <array>
#include <cstddef>
#include <new>
#include "native_fg_dx12_vulkan_platform.h"
#include "native_dx12_vulkan_platform.h"
#include "../../backends/common/frame_capture_vk.h"

// The Rust owner only needs these Vulkan result and descriptor constants.
static_assert(VK_NOT_READY == 1 && VK_INCOMPLETE == 5 && VK_ERROR_OUT_OF_HOST_MEMORY == -1 &&
    VK_ERROR_INITIALIZATION_FAILED == -3 && VK_ERROR_EXTENSION_NOT_PRESENT == -7 && VK_ERROR_FORMAT_NOT_SUPPORTED == -11 && VK_ERROR_UNKNOWN == -13 &&
    VK_ERROR_INVALID_EXTERNAL_HANDLE == -1000072003 && VK_ERROR_OUT_OF_DATE_KHR == -1000001004);
static_assert(VK_FORMAT_R8G8B8A8_UNORM == 37 && VK_FORMAT_B8G8R8A8_UNORM == 44 &&
    VK_FORMAT_R16G16_SFLOAT == 83 && VK_FORMAT_R32_SFLOAT == 100 &&
    VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL == 5 && VK_COLOR_SPACE_SRGB_NONLINEAR_KHR == 0 &&
    VK_SHARING_MODE_EXCLUSIVE == 0 && VK_IMAGE_USAGE_TRANSFER_SRC_BIT == 1 && VK_IMAGE_USAGE_SAMPLED_BIT == 4);
// The Rust transfer plan carries these layouts, queue families, masks and format features.
static_assert(VK_IMAGE_LAYOUT_UNDEFINED == 0 && VK_IMAGE_LAYOUT_GENERAL == 1 &&
    VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL == 6 && VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL == 7 &&
    VK_QUEUE_FAMILY_IGNORED == ~0u && VK_QUEUE_FAMILY_EXTERNAL == ~1u &&
    VK_ACCESS_MEMORY_READ_BIT == 0x8000 && VK_ACCESS_MEMORY_WRITE_BIT == 0x10000 &&
    VK_PIPELINE_STAGE_ALL_COMMANDS_BIT == 0x10000 &&
    VK_FORMAT_FEATURE_BLIT_SRC_BIT == 0x400 && VK_FORMAT_FEATURE_BLIT_DST_BIT == 0x800);
// The Rust swapchain admission compares these present modes and the extension sType.
static_assert(VK_PRESENT_MODE_FIFO_KHR == 2 && VK_PRESENT_MODE_FIFO_RELAXED_KHR == 3 &&
    VK_STRUCTURE_TYPE_SWAPCHAIN_LATENCY_CREATE_INFO_NV == 1000505007);
static_assert(sizeof(rsf_dx12gvk_swapchain) == 56 && offsetof(rsf_dx12gvk_swapchain, present_mode) == 36 &&
    offsetof(rsf_dx12gvk_swapchain, old_chain) == 48 && sizeof(rsf_dx12gvk_present_info) == 32 &&
    sizeof(rsf_dx12gvk_barrier) == 40 && sizeof(rsf_dx12gvk_transfer) == 32 &&
    sizeof(rsf_dx12gvk_ops) == 31 * sizeof(void*) && offsetof(rsf_dx12gvk_ops, describe) == 30 * sizeof(void*));
static_assert(offsetof(rsf_dx12gvk_ops, platform_bind) == 4 * sizeof(void*) &&
    offsetof(rsf_dx12gvk_ops, images) == 9 * sizeof(void*) &&
    sizeof(rsf_dx12gvk_dispatch_entries) == 24 * sizeof(void*));
static_assert(sizeof(rsf_dx12gvk_present_input) == 32 && offsetof(rsf_dx12gvk_present_input, wait_count) == 12);
static_assert(sizeof(rsf_dx12gvk_present_submit) == 368 && offsetof(rsf_dx12gvk_present_submit, values) == 144 &&
    offsetof(rsf_dx12gvk_present_submit, signal) == 352);

struct rsf_dx12gvk_platform {
    VkDevice device{};
    VkPhysicalDevice physical{};
    uint32_t family{};
    decltype(rsf_fgvk_setup::graphics_submit) submit{};
    decltype(rsf_fgvk_setup::lock_queues) lock{};
    decltype(rsf_fgvk_setup::unlock_queues) unlock{};
    void* queue_user{};
    rsf::FrameCaptureVk capture;
    VkPhysicalDeviceMemoryProperties memory{};
    PFN_vkGetPhysicalDeviceFormatProperties format_properties{};
    PFN_vkGetPhysicalDeviceMemoryProperties memory_properties{};
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
rsf_dx12gvk_platform* platform_allocate(const rsf_fgvk_setup* setup) {
    auto* self = new (std::nothrow) rsf_dx12gvk_platform{};
    if (!self) return nullptr;
    self->device = static_cast<VkDevice>(setup->graphics.device);
    self->physical = static_cast<VkPhysicalDevice>(setup->graphics.physical_device);
    self->family = setup->queue_family;
    self->submit = setup->graphics_submit;
    self->lock = setup->lock_queues;
    self->unlock = setup->unlock_queues;
    self->queue_user = setup->queue_user;
    return self;
}

void platform_bind(rsf_dx12gvk_platform* self, const rsf_dx12gvk_dispatch_entries* entries) {
    size_t index = 0;
#define D(field, name) self->field = reinterpret_cast<PFN_##name>((*entries)[index++])
    D(create_image, vkCreateImage); D(destroy_image, vkDestroyImage); D(requirements, vkGetImageMemoryRequirements);
    D(allocate, vkAllocateMemory); D(free_memory, vkFreeMemory); D(bind, vkBindImageMemory);
    D(create_pool, vkCreateCommandPool); D(destroy_pool, vkDestroyCommandPool); D(allocate_commands, vkAllocateCommandBuffers);
    D(reset_commands, vkResetCommandBuffer); D(begin, vkBeginCommandBuffer); D(end, vkEndCommandBuffer);
    D(barrier, vkCmdPipelineBarrier); D(copy, vkCmdCopyImage); D(blit, vkCmdBlitImage); D(clear, vkCmdClearColorImage);
    D(wait, vkWaitSemaphores); D(idle, vkDeviceWaitIdle);
    D(create_fence, vkCreateFence); D(reset_fences, vkResetFences);
    D(wait_fences, vkWaitForFences); D(destroy_fence, vkDestroyFence);
    D(memory_properties, vkGetPhysicalDeviceMemoryProperties);
    D(format_properties, vkGetPhysicalDeviceFormatProperties);
#undef D
}

uint32_t format_features(rsf_dx12gvk_platform* self, uint32_t format) {
    VkFormatProperties properties{};
    self->format_properties(self->physical, static_cast<VkFormat>(format), &properties);
    return properties.optimalTilingFeatures;
}

int32_t initialize_capture(rsf_dx12gvk_platform* self, const rsf_srvk_setup* graphics) {
    self->memory_properties(self->physical, &self->memory);
    return self->capture.initialize(self->device, graphics->device_proc, self->memory)
        ? VK_SUCCESS : VK_ERROR_INITIALIZATION_FAILED;
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
int32_t create_image(void* user, uint32_t width, uint32_t height, uint32_t format, uint32_t usage, uint64_t* output) {
    const auto& self = *static_cast<rsf_dx12gvk_platform*>(user);
    VkImageCreateInfo create{VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
    create.imageType = VK_IMAGE_TYPE_2D; create.format = static_cast<VkFormat>(format);
    create.extent = {width, height, 1}; create.mipLevels = create.arrayLayers = 1;
    create.samples = VK_SAMPLE_COUNT_1_BIT; create.tiling = VK_IMAGE_TILING_OPTIMAL;
    create.usage = usage;
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
// Rust plans layouts, queue ownership, access and stage masks, and the copy-or-blit choice.
// These leaves only translate one planned step into its typed Vulkan command.
void barrier(rsf_dx12gvk_platform* self, void* list, const rsf_dx12gvk_barrier* plan) {
    VkImageMemoryBarrier transition{VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER};
    transition.srcAccessMask = plan->source_access; transition.dstAccessMask = plan->target_access;
    transition.oldLayout = static_cast<VkImageLayout>(plan->old_layout);
    transition.newLayout = static_cast<VkImageLayout>(plan->new_layout);
    transition.srcQueueFamilyIndex = plan->source_family; transition.dstQueueFamilyIndex = plan->target_family;
    transition.image = image(plan->image); transition.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    self->barrier(static_cast<VkCommandBuffer>(list), plan->source_stages, plan->target_stages,
        0, 0, nullptr, 0, nullptr, 1, &transition);
}
void copy(rsf_dx12gvk_platform* self, void* list, const rsf_dx12gvk_transfer* plan) {
    VkImageCopy region{};
    region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    region.extent = {plan->target_width, plan->target_height, 1};
    self->copy(static_cast<VkCommandBuffer>(list), image(plan->source), VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        image(plan->target), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
}
void blit(rsf_dx12gvk_platform* self, void* list, const rsf_dx12gvk_transfer* plan) {
    VkImageBlit region{};
    region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    region.srcOffsets[1] = {static_cast<int32_t>(plan->source_width), static_cast<int32_t>(plan->source_height), 1};
    region.dstOffsets[1] = {static_cast<int32_t>(plan->target_width), static_cast<int32_t>(plan->target_height), 1};
    self->blit(static_cast<VkCommandBuffer>(list), image(plan->source), VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        image(plan->target), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region, VK_FILTER_NEAREST);
}
void clear(rsf_dx12gvk_platform* self, void* list, uint64_t target) {
    const VkClearColorValue zero{};
    const VkImageSubresourceRange range{VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    self->clear(static_cast<VkCommandBuffer>(list), image(target), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, &zero, 1, &range);
}
int32_t acquire_submit(rsf_dx12gvk_platform* self, uint64_t semaphore, uint64_t fence) {
    const auto signal = reinterpret_cast<VkSemaphore>(semaphore);
    VkSubmitInfo submit{VK_STRUCTURE_TYPE_SUBMIT_INFO};
    submit.signalSemaphoreCount = semaphore ? 1 : 0; submit.pSignalSemaphores = &signal;
    return self->submit(self->queue_user, 1, &submit, fence);
}
int32_t present_submit(rsf_dx12gvk_platform* self, void* list, const rsf_dx12gvk_present_submit* plan) {
    std::array<VkSemaphore, rsf_dx12gvk_submit_wait_capacity> waits{};
    for (uint32_t i = 0; i < plan->wait_count; ++i)
        waits[i] = reinterpret_cast<VkSemaphore>(plan->waits[i]);
    const auto signal = reinterpret_cast<VkSemaphore>(plan->signal);
    VkTimelineSemaphoreSubmitInfo timeline{VK_STRUCTURE_TYPE_TIMELINE_SEMAPHORE_SUBMIT_INFO};
    timeline.waitSemaphoreValueCount = plan->wait_count; timeline.pWaitSemaphoreValues = plan->values;
    timeline.signalSemaphoreValueCount = 1; timeline.pSignalSemaphoreValues = &plan->ready;
    const auto commands = static_cast<VkCommandBuffer>(list);
    VkSubmitInfo submit{VK_STRUCTURE_TYPE_SUBMIT_INFO}; submit.pNext = &timeline;
    submit.waitSemaphoreCount = plan->wait_count; submit.pWaitSemaphores = waits.data(); submit.pWaitDstStageMask = plan->stages;
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
// Copies facts only. Rust decides extension compatibility, present-mode vsync and old-chain refusal.
void describe(const void* create_info, rsf_dx12gvk_swapchain* out) {
    const auto& info = *static_cast<const VkSwapchainCreateInfoKHR*>(create_info);
    const auto* extra = static_cast<const VkBaseInStructure*>(info.pNext);
    *out = {info.imageExtent.width, info.imageExtent.height, static_cast<uint32_t>(info.imageFormat), info.imageUsage,
        static_cast<uint32_t>(info.imageColorSpace), info.imageArrayLayers, static_cast<uint32_t>(info.imageSharingMode),
        info.minImageCount, info.flags, static_cast<uint32_t>(info.presentMode),
        !extra ? 0u : extra->pNext ? 2u : 1u, extra ? static_cast<uint32_t>(extra->sType) : 0u,
        reinterpret_cast<uint64_t>(info.oldSwapchain)};
}
}

const rsf_dx12gvk_ops rsf_dx12gvk_graphics{
    dx12vk_platform::adapter_luid,
    platform_allocate, [](auto* self) { delete self; }, dx12vk_platform::lookup_proc,
    platform_bind, format_features, initialize_capture,
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
    barrier, copy, blit, clear, acquire_submit, present_submit, wait_timeline,
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
    describe,
};

int32_t rsf_dx12gvk_present_marshaled(rsf_dx12gvk_present_fn call, void* owner, const void* info, const rsf_frame_record* source, uint32_t explicit_source) {
    // Rust refuses a missing VkPresentInfoKHR; this reader only reports its shape.
    const auto* vk = static_cast<const VkPresentInfoKHR*>(info);
    rsf_dx12gvk_present_input input{};
    if (vk) input = {info, vk->swapchainCount, vk->waitSemaphoreCount, vk->pNext != nullptr,
        vk->pSwapchains && vk->pImageIndices, vk->pWaitSemaphores != nullptr, vk->pResults != nullptr};
    // Keep the write to pResults typed as VkResult.
    int32_t result = VK_INCOMPLETE;
    const int32_t code = rsf_core_dx12gvk_present_marshaled(call, owner, &input,
        [](const void* native, uint32_t count, uint64_t* chain, uint32_t* index, uint64_t* waits) {
            const auto& present = *static_cast<const VkPresentInfoKHR*>(native);
            *chain = reinterpret_cast<uint64_t>(present.pSwapchains[0]);
            *index = present.pImageIndices[0];
            for (uint32_t i = 0; i < count; ++i) waits[i] = reinterpret_cast<uint64_t>(present.pWaitSemaphores[i]);
        }, source, explicit_source, &result);
    if (vk && vk->pResults && result != VK_INCOMPLETE) vk->pResults[0] = static_cast<VkResult>(result);
    return code;
}
