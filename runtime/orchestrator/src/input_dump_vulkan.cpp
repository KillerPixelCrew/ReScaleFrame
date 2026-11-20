// SPDX-License-Identifier: MIT
#include <rescaleframe/input_dump_vulkan.h>
#include "../../backends/common/frame_capture_vk.h"
#include <cstddef>
#include <memory>

namespace {
using Submit = decltype(rsf_input_dump_vk_setup::submit);

struct Vulkan {
    VkDevice device = VK_NULL_HANDLE;
    rsf::FrameCaptureVk capture;
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
};

int32_t open(const rsf_srvk_setup* graphics, Vulkan** out) noexcept try {
    auto vk = std::make_unique<Vulkan>();
    vk->device = static_cast<VkDevice>(graphics->device);
#define RSF_DUMP_VK_LOAD(member, function) \
    vk->member = reinterpret_cast<PFN_##function>(graphics->device_proc(vk->device, #function)); \
    if (!vk->member) return RSF_BACKEND_ERROR_NOT_SUPPORTED
    RSF_DUMP_VK_LOAD(create_pool, vkCreateCommandPool);
    RSF_DUMP_VK_LOAD(destroy_pool, vkDestroyCommandPool);
    RSF_DUMP_VK_LOAD(allocate, vkAllocateCommandBuffers);
    RSF_DUMP_VK_LOAD(reset_pool, vkResetCommandPool);
    RSF_DUMP_VK_LOAD(begin, vkBeginCommandBuffer);
    RSF_DUMP_VK_LOAD(end, vkEndCommandBuffer);
    RSF_DUMP_VK_LOAD(create_fence, vkCreateFence);
    RSF_DUMP_VK_LOAD(destroy_fence, vkDestroyFence);
    RSF_DUMP_VK_LOAD(reset_fences, vkResetFences);
    RSF_DUMP_VK_LOAD(wait_fences, vkWaitForFences);
#undef RSF_DUMP_VK_LOAD
    const auto properties = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(
        graphics->instance_proc(graphics->instance, "vkGetPhysicalDeviceMemoryProperties"));
    if (!properties) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    VkPhysicalDeviceMemoryProperties memory{};
    properties(static_cast<VkPhysicalDevice>(graphics->physical_device), &memory);
    if (!vk->capture.initialize(vk->device, graphics->device_proc, memory) || !vk->capture.ready())
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    *out = vk.release();
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t create_pool(Vulkan* vk, uint32_t family, uint64_t* out) {
    VkCommandPoolCreateInfo info{};
    info.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
    info.queueFamilyIndex = family;
    info.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
    VkCommandPool pool = VK_NULL_HANDLE;
    const auto code = vk->create_pool(vk->device, &info, nullptr, &pool);
    if (code == VK_SUCCESS) *out = reinterpret_cast<uint64_t>(pool);
    return code;
}

int32_t allocate(Vulkan* vk, uint64_t pool, VkCommandBuffer* out) {
    VkCommandBufferAllocateInfo info{};
    info.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
    info.commandPool = reinterpret_cast<VkCommandPool>(pool);
    info.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    info.commandBufferCount = 1;
    VkCommandBuffer commands = VK_NULL_HANDLE;
    const auto code = vk->allocate(vk->device, &info, &commands);
    if (code == VK_SUCCESS) *out = commands;
    return code;
}

int32_t begin(Vulkan* vk, VkCommandBuffer commands) {
    VkCommandBufferBeginInfo info{};
    info.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
    info.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    return vk->begin(commands, &info);
}

int32_t create_fence(Vulkan* vk, uint64_t* out) {
    VkFenceCreateInfo info{};
    info.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO;
    VkFence fence = VK_NULL_HANDLE;
    const auto code = vk->create_fence(vk->device, &info, nullptr, &fence);
    if (code == VK_SUCCESS) *out = reinterpret_cast<uint64_t>(fence);
    return code;
}

int32_t submit(Submit callback, void* user, VkCommandBuffer commands, uint64_t fence) {
    if (!commands) return callback(user, 0, nullptr, fence);
    VkSubmitInfo info{};
    info.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO;
    info.commandBufferCount = 1;
    info.pCommandBuffers = &commands;
    return callback(user, 1, &info, fence);
}

int32_t record(Vulkan* vk, VkCommandBuffer commands, const rsf_srvk_image* image,
    const rsf_frame_record* frame, const char* name, uint64_t group) noexcept try {
    uint64_t ticket = 0;
    const auto recorded = vk->capture.record(commands, *image, *frame, name,
        frame->flags & RSF_FRAME_FLAG_RESET, true, &ticket, group);
    const bool added = vk->capture.pending_count() != 0;
    if (recorded) return added ? 1 : 0;
    // If the helper failed before inserting the allocation, there is no group entry to discard.
    if (!added && ticket)
        rsf_input_dump_failed(ticket, rsf::capture_name(name).c_str(), RSF_TEXTURE_ERROR_STAGING_FAILED);
    return -1;
} catch (...) { return -1; }

struct Platform {
    int32_t (*open)(const rsf_srvk_setup*, Vulkan**);
    void (*close)(Vulkan*);
    int32_t (*create_pool)(Vulkan*, uint32_t, uint64_t*);
    void (*destroy_pool)(Vulkan*, uint64_t);
    int32_t (*allocate)(Vulkan*, uint64_t, VkCommandBuffer*);
    int32_t (*reset_pool)(Vulkan*, uint64_t);
    int32_t (*begin)(Vulkan*, VkCommandBuffer);
    int32_t (*end)(Vulkan*, VkCommandBuffer);
    int32_t (*create_fence)(Vulkan*, uint64_t*);
    void (*destroy_fence)(Vulkan*, uint64_t);
    int32_t (*reset_fence)(Vulkan*, uint64_t);
    int32_t (*wait_fence)(Vulkan*, uint64_t, uint64_t);
    int32_t (*submit)(Submit, void*, VkCommandBuffer, uint64_t);
    int32_t (*wants)(const rsf_frame_record*);
    int32_t (*has_readbacks)(Vulkan*);
    int32_t (*record)(Vulkan*, VkCommandBuffer, const rsf_srvk_image*, const rsf_frame_record*, const char*, uint64_t);
    void (*complete)(Vulkan*, uint64_t);
    void (*discard)(Vulkan*, uint64_t);
    int32_t (*flush)(Vulkan*);
    uint64_t (*watermark)(const rsf_frame_record*);
    void (*publish_watermark)(void*, uint64_t);
};
static_assert(sizeof(Platform) == 21 * sizeof(void*));
static_assert(sizeof(rsf_input_dump_vk_setup) == 128 && offsetof(rsf_input_dump_vk_setup, submit) == 112);

const Platform platform{
    open,
    [](Vulkan* vk) { delete vk; },
    create_pool,
    [](Vulkan* vk, uint64_t pool) { vk->destroy_pool(vk->device, reinterpret_cast<VkCommandPool>(pool), nullptr); },
    allocate,
    [](Vulkan* vk, uint64_t pool) -> int32_t { return vk->reset_pool(vk->device, reinterpret_cast<VkCommandPool>(pool), 0); },
    begin,
    [](Vulkan* vk, VkCommandBuffer commands) -> int32_t { return vk->end(commands); },
    create_fence,
    [](Vulkan* vk, uint64_t fence) { vk->destroy_fence(vk->device, reinterpret_cast<VkFence>(fence), nullptr); },
    [](Vulkan* vk, uint64_t fence) -> int32_t {
        const auto handle = reinterpret_cast<VkFence>(fence);
        return vk->reset_fences(vk->device, 1, &handle);
    },
    [](Vulkan* vk, uint64_t fence, uint64_t timeout) -> int32_t {
        const auto handle = reinterpret_cast<VkFence>(fence);
        return vk->wait_fences(vk->device, 1, &handle, VK_TRUE, timeout);
    },
    submit, rsf_input_dump_wants,
    [](Vulkan* vk) noexcept -> int32_t { try { return vk->capture.pending_count() != 0; } catch (...) { return 1; } },
    record,
    [](Vulkan* vk, uint64_t group) noexcept { try { vk->capture.complete_group(group); } catch (...) {} },
    [](Vulkan* vk, uint64_t group) noexcept { try { vk->capture.discard_group(group); } catch (...) {} },
    [](Vulkan* vk) -> int32_t { return vk->capture.flush(); },
    rsf_input_dump_gpu_submission,
    [](void* device, uint64_t watermark) { rsf_input_dump_gpu_completed(watermark, 3, device); },
};
}

extern "C" int32_t rsf_core_input_dump_vk_create(const rsf_input_dump_vk_setup*, rsf_input_dump_vk**, const Platform*);
extern "C" int32_t rsf_core_input_dump_vk_snapshot(rsf_input_dump_vk*, const rsf_frame_record*, const rsf_srvk_image*, const char*);
extern "C" int32_t rsf_core_input_dump_vk_drain(rsf_input_dump_vk*);
extern "C" int32_t rsf_core_input_dump_vk_destroy(rsf_input_dump_vk*);

extern "C" RSF_RUNTIME_API int32_t rsf_input_dump_vk_create(const rsf_input_dump_vk_setup* setup, rsf_input_dump_vk** out) {
    return rsf_core_input_dump_vk_create(setup, out, &platform);
}
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump_vk_snapshot(rsf_input_dump_vk* owner, const rsf_frame_record* frame,
    const rsf_srvk_image* image, const char* name) {
    return rsf_core_input_dump_vk_snapshot(owner, frame, image, name);
}
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump_vk_drain(rsf_input_dump_vk* owner) {
    return rsf_core_input_dump_vk_drain(owner);
}
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump_vk_destroy(rsf_input_dump_vk* owner) {
    return rsf_core_input_dump_vk_destroy(owner);
}
