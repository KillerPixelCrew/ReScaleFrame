// SPDX-License-Identifier: MIT
#include <rescaleframe/input_dump.h>
#include <rescaleframe/input_dump_vulkan.h>
#include <windows.h>
#include <vulkan/vulkan.h>
#include <array>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>

namespace {
bool passed = true;
void check(bool okay, const char* message) {
    if (!okay) { std::fprintf(stderr, "%s\n", message); passed = false; }
}

struct Readback {
    bool live = false;
    bool memory = false;
    bool copied = false;
    std::array<unsigned char, 4> bytes{64, 128, 255, 0};
};

struct Queue {
    std::array<Readback, 16> readbacks{};
    size_t buffers = 0, frees = 0, maps = 0, invalid_maps = 0;
    size_t pools = 0, pool_destroys = 0, fence_destroys = 0, resets = 0;
    size_t submits = 0, markers = 0, waits = 0, copies_recorded = 0;
    uint64_t timeout = 0;
    bool fence_reset = false, pending = false, ready = true, signaled = false;
    bool missing_wait = false, fail_fence = false, fail_bind = false, fail_end = false;
    VkResult submit_result = VK_SUCCESS;
    Readback* recording = nullptr;
    Readback* queued = nullptr;

    auto device() { return reinterpret_cast<VkDevice>(this); }
    auto commands() { return reinterpret_cast<VkCommandBuffer>(this); }
    auto pool() { return reinterpret_cast<VkCommandPool>(&pools); }
    auto fence() { return reinterpret_cast<VkFence>(&fence_reset); }
    auto& next() { return readbacks.at(buffers++); }
    auto& last() { return readbacks.at(buffers - 1); }
};

Queue& queue(VkDevice device) { return *reinterpret_cast<Queue*>(device); }
Queue& queue(VkCommandBuffer commands) { return *reinterpret_cast<Queue*>(commands); }

VKAPI_ATTR VkResult VKAPI_CALL create_pool(VkDevice device, const VkCommandPoolCreateInfo* info,
    const VkAllocationCallbacks*, VkCommandPool* out) {
    auto& q = queue(device);
    check(info->queueFamilyIndex == 7 && info->flags == VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT,
        "Use the engine queue family for the isolated pool.");
    ++q.pools;
    *out = q.pool();
    return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL destroy_pool(VkDevice device, VkCommandPool pool, const VkAllocationCallbacks*) {
    auto& q = queue(device);
    check(pool == q.pool() && !q.pending, "Destroy the pool only after its fence retires.");
    ++q.pool_destroys;
}
VKAPI_ATTR VkResult VKAPI_CALL allocate_commands(VkDevice device, const VkCommandBufferAllocateInfo* info,
    VkCommandBuffer* out) {
    auto& q = queue(device);
    check(info->commandPool == q.pool() && info->level == VK_COMMAND_BUFFER_LEVEL_PRIMARY &&
        info->commandBufferCount == 1, "Allocate a single primary command buffer from the owned pool.");
    *out = q.commands();
    return VK_SUCCESS;
}
VKAPI_ATTR VkResult VKAPI_CALL reset_pool(VkDevice device, VkCommandPool pool, VkCommandPoolResetFlags) {
    auto& q = queue(device);
    check(pool == q.pool() && !q.pending, "Never reset a pending pool.");
    ++q.resets;
    q.recording = nullptr;
    return VK_SUCCESS;
}
VKAPI_ATTR VkResult VKAPI_CALL begin(VkCommandBuffer, const VkCommandBufferBeginInfo* info) {
    check(info->flags == VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT, "Record a one-time snapshot.");
    return VK_SUCCESS;
}
VKAPI_ATTR VkResult VKAPI_CALL end(VkCommandBuffer commands) {
    return queue(commands).fail_end ? VK_ERROR_OUT_OF_HOST_MEMORY : VK_SUCCESS;
}
VKAPI_ATTR VkResult VKAPI_CALL create_fence(VkDevice device, const VkFenceCreateInfo* info,
    const VkAllocationCallbacks*, VkFence* out) {
    auto& q = queue(device);
    check(info->flags == 0, "Create an unsignaled fence.");
    if (q.fail_fence) return VK_ERROR_OUT_OF_HOST_MEMORY;
    *out = q.fence();
    return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL destroy_fence(VkDevice device, VkFence fence, const VkAllocationCallbacks*) {
    auto& q = queue(device);
    check(fence == q.fence() && !q.pending, "Do not destroy a pending fence.");
    ++q.fence_destroys;
}
VKAPI_ATTR VkResult VKAPI_CALL reset_fences(VkDevice device, uint32_t count, const VkFence* fences) {
    auto& q = queue(device);
    check(count == 1 && fences[0] == q.fence() && !q.pending, "Never reset a pending fence.");
    q.signaled = false;
    q.fence_reset = true;
    return VK_SUCCESS;
}
VKAPI_ATTR VkResult VKAPI_CALL wait_fences(VkDevice device, uint32_t count, const VkFence* fences,
    VkBool32 all, uint64_t timeout) {
    auto& q = queue(device);
    check(count == 1 && fences[0] == q.fence() && all == VK_TRUE, "Wait on the exact submitted fence.");
    ++q.waits;
    q.timeout = timeout;
    if (q.pending && q.ready) {
        if (q.queued) q.queued->copied = true;
        q.queued = nullptr;
        q.pending = false;
        q.signaled = true;
    }
    return q.signaled ? VK_SUCCESS : VK_TIMEOUT;
}

int32_t submit(void* user, uint32_t count, const void* values, uint64_t fence) {
    auto& q = *static_cast<Queue*>(user);
    check(fence == reinterpret_cast<uint64_t>(q.fence()) && q.fence_reset && !q.pending && !q.signaled,
        "The engine callback receives the fresh fence on its serialized queue.");
    q.fence_reset = false;
    if (count == 1 && values) {
        const auto& info = *static_cast<const VkSubmitInfo*>(values);
        check(info.sType == VK_STRUCTURE_TYPE_SUBMIT_INFO && info.commandBufferCount == 1 &&
            info.pCommandBuffers[0] == q.commands() && info.waitSemaphoreCount == 0 &&
            info.signalSemaphoreCount == 0 && q.recording, "Submit only the copy command buffer, without Present waits.");
        ++q.submits;
    } else {
        check(count == 0 && !values, "Drain with a fence-only engine submit.");
        ++q.markers;
    }
    if (q.submit_result == VK_SUCCESS || q.submit_result == VK_ERROR_DEVICE_LOST) {
        q.queued = count ? q.recording : nullptr;
        q.pending = true;
    }
    return q.submit_result;
}

VKAPI_ATTR void VKAPI_CALL properties(VkPhysicalDevice, VkPhysicalDeviceMemoryProperties* out) {
    out->memoryTypeCount = 1;
    out->memoryTypes[0].propertyFlags = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
}
VKAPI_ATTR VkResult VKAPI_CALL create_buffer(VkDevice device, const VkBufferCreateInfo* info,
    const VkAllocationCallbacks*, VkBuffer* out) {
    check(info->size == 4 && info->usage == VK_BUFFER_USAGE_TRANSFER_DST_BIT, "Read back one RGBA pixel.");
    auto& readback = queue(device).next();
    readback.live = true;
    *out = reinterpret_cast<VkBuffer>(&readback);
    return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL destroy_buffer(VkDevice device, VkBuffer buffer, const VkAllocationCallbacks*) {
    auto& readback = *reinterpret_cast<Readback*>(buffer);
    auto& q = queue(device);
    check(readback.live && q.queued != &readback, "Release readback buffers once, after actual retirement.");
    readback.live = false;
}
VKAPI_ATTR void VKAPI_CALL requirements(VkDevice, VkBuffer, VkMemoryRequirements* out) {
    *out = {4, 1, 1};
}
VKAPI_ATTR VkResult VKAPI_CALL allocate_memory(VkDevice device, const VkMemoryAllocateInfo*,
    const VkAllocationCallbacks*, VkDeviceMemory* out) {
    auto& readback = queue(device).last();
    readback.memory = true;
    *out = reinterpret_cast<VkDeviceMemory>(&readback);
    return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL free_memory(VkDevice device, VkDeviceMemory memory, const VkAllocationCallbacks*) {
    auto& q = queue(device);
    auto& readback = *reinterpret_cast<Readback*>(memory);
    check(readback.memory && q.queued != &readback, "Never free a submitted readback on timeout.");
    readback.memory = false;
    ++q.frees;
}
VKAPI_ATTR VkResult VKAPI_CALL bind(VkDevice device, VkBuffer, VkDeviceMemory, VkDeviceSize) {
    return queue(device).fail_bind ? VK_ERROR_OUT_OF_DEVICE_MEMORY : VK_SUCCESS;
}
VKAPI_ATTR VkResult VKAPI_CALL map(VkDevice device, VkDeviceMemory memory, VkDeviceSize offset, VkDeviceSize size,
    VkMemoryMapFlags, void** out) {
    auto& q = queue(device);
    auto& readback = *reinterpret_cast<Readback*>(memory);
    check(readback.live && readback.memory && offset == 0 && size == 4, "Map the exact readback allocation.");
    ++q.maps;
    if (!readback.copied) {
        ++q.invalid_maps;
        readback.bytes.fill(0xef);
    }
    *out = readback.bytes.data();
    return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL unmap(VkDevice, VkDeviceMemory) {}
VKAPI_ATTR void VKAPI_CALL barrier(VkCommandBuffer, VkPipelineStageFlags, VkPipelineStageFlags, VkDependencyFlags,
    uint32_t, const VkMemoryBarrier*, uint32_t, const VkBufferMemoryBarrier*, uint32_t, const VkImageMemoryBarrier*) {}
VKAPI_ATTR void VKAPI_CALL copy(VkCommandBuffer commands, VkImage, VkImageLayout, VkBuffer buffer,
    uint32_t, const VkBufferImageCopy*) {
    auto& q = queue(commands);
    q.recording = reinterpret_cast<Readback*>(buffer);
    ++q.copies_recorded;
}

rsf_vk_function device_proc(void* device, const char* name) {
#define PROC(vulkan, leaf) if (std::strcmp(name, #vulkan) == 0) return reinterpret_cast<rsf_vk_function>(leaf)
    PROC(vkCreateCommandPool, create_pool);
    PROC(vkDestroyCommandPool, destroy_pool);
    PROC(vkAllocateCommandBuffers, allocate_commands);
    PROC(vkResetCommandPool, reset_pool);
    PROC(vkBeginCommandBuffer, begin);
    PROC(vkEndCommandBuffer, end);
    PROC(vkCreateFence, create_fence);
    PROC(vkDestroyFence, destroy_fence);
    PROC(vkResetFences, reset_fences);
    if (!static_cast<Queue*>(device)->missing_wait) { PROC(vkWaitForFences, wait_fences); }
    PROC(vkCreateBuffer, create_buffer);
    PROC(vkDestroyBuffer, destroy_buffer);
    PROC(vkGetBufferMemoryRequirements, requirements);
    PROC(vkAllocateMemory, allocate_memory);
    PROC(vkFreeMemory, free_memory);
    PROC(vkBindBufferMemory, bind);
    PROC(vkMapMemory, map);
    PROC(vkUnmapMemory, unmap);
    PROC(vkCmdPipelineBarrier, barrier);
    PROC(vkCmdCopyImageToBuffer, copy);
    return nullptr;
}
rsf_vk_function instance_proc(void*, const char* name) {
    PROC(vkGetPhysicalDeviceMemoryProperties, properties);
    return nullptr;
#undef PROC
}

std::string read(const std::filesystem::path& file) {
    std::ifstream stream(file, std::ios::binary);
    return {std::istreambuf_iterator<char>(stream), {}};
}
}

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    const auto root = std::filesystem::path(argv[1]) / ("vulkan-cpu-" +
        std::to_string(GetCurrentProcessId()) + '-' + std::to_string(GetTickCount64()));
    const auto legacy = root / "legacy";
    SetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", legacy.c_str());
    SetEnvironmentVariableW(L"RSF_FRAME_CAPTURE_FIRST", L"32");
    SetEnvironmentVariableW(L"RSF_FRAME_CAPTURE_COUNT", L"1");
    Queue q;
    rsf_input_dump_vk_setup setup{sizeof(setup), 1, {}, &q, 7, submit, &q};
    setup.graphics.struct_size = sizeof(setup.graphics);
    setup.graphics.abi_version = RSF_SRVK_ABI_VERSION;
    setup.graphics.instance = setup.graphics.device = setup.graphics.physical_device = &q;
    setup.graphics.instance_proc = instance_proc;
    setup.graphics.device_proc = device_proc;
    rsf_input_dump_vk* owner = nullptr;
    check(rsf_input_dump_vk_create(&setup, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "Refuse a null output.");
    auto short_setup = setup;
    short_setup.struct_size = sizeof(uint32_t);
    check(rsf_input_dump_vk_create(&short_setup, &owner) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !owner,
        "Refuse a short setup before Vulkan calls.");
    q.missing_wait = true;
    check(rsf_input_dump_vk_create(&setup, &owner) == RSF_BACKEND_ERROR_NOT_SUPPORTED && !owner && !q.pools,
        "Refuse incomplete Vulkan dispatch before allocation.");
    q.missing_wait = false;
    q.fail_fence = true;
    check(rsf_input_dump_vk_create(&setup, &owner) == RSF_BACKEND_ERROR_INIT_FAILED && !owner &&
        q.pools == 1 && q.pool_destroys == 1 && !q.fence_destroys, "Clean up a partial setup.");
    q.fail_fence = false;
    if (rsf_input_dump_vk_create(&setup, &owner) != RSF_BACKEND_OK || !owner) return 1;

    rsf_frame_record frame{};
    frame.struct_size = sizeof(frame);
    frame.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    frame.session_id = 31; frame.frame_id = 32; frame.view_id = 33;
    frame.render_width = frame.render_height = frame.output_width = frame.output_height = 1;
    const rsf_srvk_image image{q.device(), 1, VK_FORMAT_R8G8B8A8_UNORM, 1, 1, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
    check(rsf_input_dump_vk_snapshot(owner, &frame, &image, "unrequested") == RSF_BACKEND_OK &&
        !q.resets && !q.submits && !q.maps && !q.waits && !q.buffers, "Unrequested frames do no GPU work even with legacy output enabled.");
    rsf_input_dump_configure(root.string().c_str(), "Vulkan CPU fixture", "fixture", nullptr, nullptr);
    rsf_input_dump_request();
    check(rsf_input_dump_begin_record(&frame, 2) != 0, "Arm the shared Rust capture owner.");
    auto unsupported = image;
    unsupported.copy_source = 0;
    check(rsf_input_dump_vk_snapshot(owner, &frame, &unsupported, "unsupported") == RSF_BACKEND_ERROR_INVALID_ARGUMENT &&
        !q.resets && !q.buffers && !q.submits, "Refuse incompatible images before starting a recording.");
    const auto engine_ticket = rsf_input_dump_reserve(&frame, "engine", "Engine readback", RSF_DUMP_VIEW_HDR, 1.0f);
    check(engine_ticket != 0, "Reserve an earlier same-queue stage.");

    q.fail_end = true;
    check(rsf_input_dump_vk_snapshot(owner, &frame, &image, "refused_end") == RSF_BACKEND_ERROR_FEATURE_FAILED &&
        !q.submits && q.copies_recorded == 1 && q.frees == 1 && !q.last().live, "Discard a copy whose command buffer could not end.");
    q.fail_end = false;
    const auto poisoned = rsf_input_dump_gpu_submission(&frame);
    check(poisoned != 0, "Construct a same-frame shared completion token.");
    rsf_input_dump_gpu_completed(poisoned, 3, q.device());
    check(rsf_input_dump_vk_snapshot(owner, &frame, &image, "successful") == RSF_BACKEND_OK &&
        q.submits == 1 && q.maps == 1 && !q.invalid_maps && q.frees == 2 && q.timeout == 15000000000ull,
        "Only the successful recording publishes after its own bounded fence wait.");

    q.submit_result = VK_ERROR_OUT_OF_DEVICE_MEMORY;
    const auto prior_waits = q.waits;
    check(rsf_input_dump_vk_snapshot(owner, &frame, &image, "refused_submit") == RSF_BACKEND_ERROR_FEATURE_FAILED &&
        q.submits == 2 && q.waits == prior_waits && q.maps == 1 && q.frees == 3,
        "A rejected submission cannot reuse the previous fence signal.");
    q.submit_result = VK_SUCCESS;
    check(rsf_input_dump_vk_drain(owner) == RSF_BACKEND_OK && q.markers == 1 && q.maps == 1,
        "A subsequent empty drain cannot publish rejected recordings.");

    q.fail_bind = true;
    check(rsf_input_dump_vk_snapshot(owner, &frame, &image, "refused_bind") == RSF_BACKEND_ERROR_FEATURE_FAILED &&
        q.submits == 2 && q.copies_recorded == 3 && q.frees == 4 && !q.last().live,
        "Free a partially allocated recording without submitting.");
    q.fail_bind = false;
    const auto later_ticket = rsf_input_dump_reserve(&frame, "later_engine", "Later engine readback", RSF_DUMP_VIEW_HDR, 1.0f);
    check(later_ticket != 0, "Reserve another engine stage after the earlier fence.");
    q.ready = false;
    check(rsf_input_dump_vk_snapshot(owner, &frame, &image, "final_color") == RSF_BACKEND_ERROR_NOT_READY &&
        q.pending && q.last().live && q.last().memory && q.maps == 1 && q.frees == 4, "Keep the actual submitted allocation through timeout.");
    const auto stalled_waits = q.waits;
    auto absent = frame;
    ++absent.frame_id;
    check(rsf_input_dump_vk_snapshot(owner, &absent, &image, "absent") == RSF_BACKEND_OK && q.waits == stalled_waits,
        "A different frame does no GPU work while the admitted capture is pending.");
    const auto stalled_resets = q.resets;
    check(rsf_input_dump_vk_snapshot(owner, &frame, &image, "blocked") == RSF_BACKEND_ERROR_NOT_READY &&
        q.resets == stalled_resets && q.timeout == 0 && q.submits == 3, "A later snapshot polls without overwriting pending commands.");
    check(rsf_input_dump_vk_destroy(owner) == RSF_BACKEND_ERROR_NOT_READY && !q.fence_destroys && q.pool_destroys == 1,
        "A timed-out destroy preserves the owner.");
    check(rsf_input_dump_vk_drain(owner) == RSF_BACKEND_ERROR_NOT_READY && q.markers == 1 && q.maps == 1 && q.frees == 4,
        "Drain does not issue another submit until the exact pending fence retires.");
    check(!rsf_input_dump_gpu_ready(later_ticket, "later_engine", 3, q.device()),
        "A stale watermark cannot become evidence for the later stage.");
    q.ready = true;
    check(rsf_input_dump_vk_drain(owner) == RSF_BACKEND_OK && q.markers == 2 && q.maps == 2 &&
        !q.invalid_maps && q.frees == 5 && !q.pending, "Retry retires the original submitted readback exactly once.");
    check(rsf_input_dump_gpu_ready(later_ticket, "later_engine", 3, q.device()) != 0,
        "A completed snapshot still certifies the earlier engine stages on its queue.");
    rsf_input_dump_failed(engine_ticket, "engine", RSF_TEXTURE_ERROR_STAGING_FAILED);
    rsf_input_dump_failed(later_ticket, "later_engine", RSF_TEXTURE_ERROR_STAGING_FAILED);
    rsf_input_dump_complete(&frame, 0);

    std::filesystem::path bundle;
    for (const auto& entry : std::filesystem::directory_iterator(root / "website")) bundle = entry.path();
    const std::string pixel("\x40\x80\xff\0", 4);
    for (const char* accepted : {"successful", "final_color"}) {
        check(read(bundle / (std::string(accepted) + ".bin")) == pixel, "Publish exact pixels through the shared capture owner.");
        check(read(legacy / (std::string("31-32-") + accepted + "-0.raw")) == pixel, "Publish matching pixels through optional legacy output.");
    }
    for (const char* refused : {"refused_bind", "refused_end", "refused_submit", "blocked"}) {
        check(!std::filesystem::exists(bundle / (std::string(refused) + ".bin")) &&
            !std::filesystem::exists(legacy / (std::string("31-32-") + refused + "-0.raw")),
            "Publish neither core nor legacy files for abandoned work.");
    }

    q.ready = false;
    check(rsf_input_dump_vk_drain(owner) == RSF_BACKEND_ERROR_NOT_READY && q.markers == 3 && q.pending,
        "Keep the submitted empty drain fence through timeout.");
    check(rsf_input_dump_vk_destroy(owner) == RSF_BACKEND_ERROR_NOT_READY && !q.fence_destroys,
        "The pending empty submit also blocks destruction.");
    q.ready = true;
    check(rsf_input_dump_vk_destroy(owner) == RSF_BACKEND_OK && q.pool_destroys == 2 && q.fence_destroys == 1 &&
        q.markers == 3 && q.maps == 2 && q.buffers == q.frees, "Destroy retires the empty fence without publishing stale evidence.");
    check(rsf_input_dump_vk_drain(nullptr) == RSF_BACKEND_OK && rsf_input_dump_vk_destroy(nullptr) == RSF_BACKEND_OK,
        "Keep null drain and destroy idempotent.");

    if (rsf_input_dump_vk_create(&setup, &owner) != RSF_BACKEND_OK || !owner) return 1;
    rsf_input_dump_request();
    check(rsf_input_dump_begin_record(&frame, 2) != 0, "Arm a new request even for a reused frame identity.");
    q.ready = false;
    q.submit_result = VK_ERROR_DEVICE_LOST;
    check(rsf_input_dump_vk_snapshot(owner, &frame, &image, "uncertain_submit") == RSF_BACKEND_ERROR_FEATURE_FAILED &&
        q.pending && q.last().live && q.frees == 5, "A device-loss submit may leave work in use without valid pixels.");
    const auto blocked_destroy = rsf_input_dump_vk_destroy(owner);
    check(blocked_destroy == RSF_BACKEND_ERROR_NOT_READY,
        "An uncertain submission retains the owner when the fence has no completion.");
    if (blocked_destroy != RSF_BACKEND_ERROR_NOT_READY) return 1;
    q.ready = true;
    check(rsf_input_dump_vk_destroy(owner) == RSF_BACKEND_OK && q.maps == 2 && q.frees == 6 &&
        !std::filesystem::exists(legacy / "31-32-uncertain_submit-0.raw"),
        "Retire uncertain work without allowing its fence to publish discarded data.");
    rsf_input_dump_cancel(0);
    return passed ? 0 : 1;
}
