// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/input_dump_vulkan.h>
#include "../../backends/common/frame_capture_vk.h"
#include <memory>

struct rsf_input_dump_vk {
    rsf_input_dump_vk_setup setup{};
    rsf::FrameCaptureVk capture;
    VkCommandPool pool = VK_NULL_HANDLE;
    VkCommandBuffer commands = VK_NULL_HANDLE;
    VkFence fence = VK_NULL_HANDLE;
    bool pending = false;
    uint64_t submission = 0;
    PFN_vkCreateCommandPool create_pool = nullptr;
    PFN_vkDestroyCommandPool destroy_pool = nullptr;
    PFN_vkAllocateCommandBuffers allocate = nullptr;
    PFN_vkResetCommandPool reset_pool = nullptr;
    PFN_vkBeginCommandBuffer begin = nullptr;
    PFN_vkEndCommandBuffer end = nullptr;
    PFN_vkCreateFence create_fence = nullptr;
    PFN_vkDestroyFence destroy_fence = nullptr;
    PFN_vkResetFences reset_fences = nullptr;
    PFN_vkWaitForFences wait_fences = nullptr;
    ~rsf_input_dump_vk() {
        const auto device = static_cast<VkDevice>(setup.graphics.device);
        if (pool) destroy_pool(device, pool, nullptr);
        if (fence) destroy_fence(device, fence, nullptr);
    }
};
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump_vk_create(const rsf_input_dump_vk_setup* setup, rsf_input_dump_vk** out) try
{
    if (!out) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    if (!setup || setup->struct_size < sizeof(*setup) || setup->abi_version != 1 || setup->graphics.struct_size < sizeof(setup->graphics) ||
        setup->graphics.abi_version != RSF_SRVK_ABI_VERSION || !setup->graphics.instance || !setup->graphics.device ||
        !setup->graphics.physical_device || !setup->graphics.instance_proc || !setup->graphics.device_proc ||
        !setup->queue || !setup->submit) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto self = std::make_unique<rsf_input_dump_vk>(); self->setup = *setup;
    const auto device = static_cast<VkDevice>(setup->graphics.device);
#define RSF_DUMP_VK_LOAD(member, function) self->member = reinterpret_cast<PFN_##function>(setup->graphics.device_proc(device, #function)); if (!self->member) return RSF_BACKEND_ERROR_NOT_SUPPORTED
    RSF_DUMP_VK_LOAD(create_pool, vkCreateCommandPool); RSF_DUMP_VK_LOAD(destroy_pool, vkDestroyCommandPool);
    RSF_DUMP_VK_LOAD(allocate, vkAllocateCommandBuffers); RSF_DUMP_VK_LOAD(reset_pool, vkResetCommandPool);
    RSF_DUMP_VK_LOAD(begin, vkBeginCommandBuffer); RSF_DUMP_VK_LOAD(end, vkEndCommandBuffer);
    RSF_DUMP_VK_LOAD(create_fence, vkCreateFence); RSF_DUMP_VK_LOAD(destroy_fence, vkDestroyFence);
    RSF_DUMP_VK_LOAD(reset_fences, vkResetFences); RSF_DUMP_VK_LOAD(wait_fences, vkWaitForFences);
#undef RSF_DUMP_VK_LOAD
    const auto properties = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(setup->graphics.instance_proc(setup->graphics.instance, "vkGetPhysicalDeviceMemoryProperties"));
    if (!properties) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    VkPhysicalDeviceMemoryProperties memory{}; properties(static_cast<VkPhysicalDevice>(setup->graphics.physical_device), &memory);
    if (!self->capture.initialize(device, setup->graphics.device_proc, memory) || !self->capture.ready()) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    VkCommandPoolCreateInfo pool{}; pool.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
    pool.queueFamilyIndex = setup->queue_family; pool.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
    VkCommandBufferAllocateInfo allocation{}; allocation.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
    allocation.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY; allocation.commandBufferCount = 1;
    if (self->create_pool(device, &pool, nullptr, &self->pool) != VK_SUCCESS) return RSF_BACKEND_ERROR_INIT_FAILED;
    allocation.commandPool = self->pool;
    VkFenceCreateInfo fence{}; fence.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO;
    if (self->allocate(device, &allocation, &self->commands) != VK_SUCCESS || self->create_fence(device, &fence, nullptr, &self->fence) != VK_SUCCESS)
        return RSF_BACKEND_ERROR_INIT_FAILED;
    *out = self.release(); return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump_vk_snapshot(rsf_input_dump_vk* self, const rsf_frame_record* record,
    const rsf_srvk_image* image, const char* name) try
{
    if (!self || !record || !image || !name) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!rsf_input_dump_wants(record)) return RSF_BACKEND_OK;
    const auto device = static_cast<VkDevice>(self->setup.graphics.device);
    if (self->pending) {
        if (self->wait_fences(device, 1, &self->fence, VK_TRUE, 0) != VK_SUCCESS) return RSF_BACKEND_ERROR_NOT_READY;
        self->pending = false; rsf_input_dump_gpu_completed(self->submission, 3, device); (void)self->capture.flush();
    }
    if (image->device != device || !image->copy_source || !image->image) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (self->reset_pool(device, self->pool, 0) != VK_SUCCESS || self->reset_fences(device, 1, &self->fence) != VK_SUCCESS)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    VkCommandBufferBeginInfo begin{}; begin.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO; begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    const auto previous = self->capture.pending_count();
    uint64_t image_ticket = 0;
    if (self->begin(self->commands, &begin) != VK_SUCCESS || !self->capture.record(self->commands, *image, *record, name, record->flags & RSF_FRAME_FLAG_RESET, true, &image_ticket) ||
        self->end(self->commands) != VK_SUCCESS) {
        rsf_input_dump_failed(image_ticket, rsf::capture_name(name).c_str(), RSF_TEXTURE_ERROR_STAGING_FAILED); return RSF_BACKEND_ERROR_FEATURE_FAILED;
    }
    if (self->capture.pending_count() == previous) return RSF_BACKEND_OK;
    VkSubmitInfo submit{}; submit.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO; submit.commandBufferCount = 1; submit.pCommandBuffers = &self->commands;
    const auto submission = rsf_input_dump_gpu_submission(record);
    if (self->setup.submit(self->setup.user, 1, &submit, reinterpret_cast<uint64_t>(self->fence)) != VK_SUCCESS) {
        rsf_input_dump_failed(image_ticket, rsf::capture_name(name).c_str(), RSF_TEXTURE_ERROR_STAGING_FAILED); return RSF_BACKEND_ERROR_FEATURE_FAILED;
    }
    self->submission = submission;
    self->pending = true;
    if (self->wait_fences(device, 1, &self->fence, VK_TRUE, 15000000000ull) != VK_SUCCESS) return RSF_BACKEND_ERROR_NOT_READY;
    self->pending = false;
    rsf_input_dump_gpu_completed(self->submission, 3, device);
    return self->capture.flush() ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump_vk_destroy(rsf_input_dump_vk* self)
{
    if (!self) return RSF_BACKEND_OK;
    if (self->pending && self->wait_fences(static_cast<VkDevice>(self->setup.graphics.device), 1, &self->fence, VK_TRUE, 0) != VK_SUCCESS)
        return RSF_BACKEND_ERROR_NOT_READY;
    rsf_input_dump_gpu_completed(self->submission, 3, self->setup.graphics.device);
    (void)self->capture.flush(); delete self; return RSF_BACKEND_OK;
}
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump_vk_drain(rsf_input_dump_vk* self) try
{
    if (!self) return RSF_BACKEND_OK;
    const auto device = static_cast<VkDevice>(self->setup.graphics.device);
    if (self->pending) {
        if (self->wait_fences(device, 1, &self->fence, VK_TRUE, 15000000000ull) != VK_SUCCESS) return RSF_BACKEND_ERROR_NOT_READY;
        self->pending = false;
    }
    rsf_input_dump_gpu_completed(self->submission, 3, device);
    if (self->reset_fences(device, 1, &self->fence) != VK_SUCCESS ||
        self->setup.submit(self->setup.user, 0, nullptr, reinterpret_cast<uint64_t>(self->fence)) != VK_SUCCESS) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self->pending = true;
    if (self->wait_fences(device, 1, &self->fence, VK_TRUE, 15000000000ull) != VK_SUCCESS) return RSF_BACKEND_ERROR_NOT_READY;
    self->pending = false; return self->capture.flush() ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
