// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
#include <rescaleframe/capture_names.h>
#include <windows.h>
#include <vulkan/vulkan.h>
#include <algorithm>
#include <cstring>
#include <cwchar>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <memory>
#include <vector>
#include <mutex>

// Rust (runtime/orchestrator/src/frame_capture_vk.rs) owns selection, capture
// records, completion proof, pending readback lifetime and publishing. These
// native leaves only issue Vulkan buffer, memory, event and command calls.
extern "C" {
struct rsf_core_capture_vk;
struct rsf_core_capture_vk_allocation { uint64_t buffer, memory, event; };
struct rsf_core_capture_vk_platform {
    int32_t (*allocate)(void* native, const rsf_srvk_image*, uint64_t bytes, uint32_t event, rsf_core_capture_vk_allocation*);
    void (*encode)(void* native, void* commands, const rsf_srvk_image*, const rsf_core_capture_vk_allocation*);
    int32_t (*event_set)(void* native, uint64_t event);
    int32_t (*map)(void* native, uint64_t memory, uint64_t bytes, void** data);
    void (*unmap)(void* native, uint64_t memory);
    void (*release)(void* native, const rsf_core_capture_vk_allocation*);
    int (*wants)(const rsf_frame_record*);
    int (*wants_frame)(uint64_t);
    uint64_t (*reserve)(const rsf_frame_record*, const char*, const char*, rsf_dump_view, float);
    void (*failed)(uint64_t, const char*, int32_t);
    int (*pixels)(uint64_t, const char*, const rsf_dump_pixels*, uint32_t, uint32_t);
    int (*gpu_ready)(uint64_t, const char*, uint32_t, void*);
};
static_assert(sizeof(rsf_core_capture_vk_platform) == 12 * sizeof(void*));
static_assert(sizeof(rsf_core_capture_vk_allocation) == 24 && sizeof(rsf_dump_pixels) == 40);
rsf_core_capture_vk* rsf_core_capture_vk_create(uint32_t command_completion, void* native, const rsf_core_capture_vk_platform*);
void rsf_core_capture_vk_destroy(rsf_core_capture_vk*);
int32_t rsf_core_capture_vk_enabled(const rsf_core_capture_vk*);
void rsf_core_capture_vk_bind(const rsf_core_capture_vk*, void* device);
uint64_t rsf_core_capture_vk_pending(const rsf_core_capture_vk*);
void rsf_core_capture_vk_provider(const rsf_core_capture_vk*, uint64_t version, uint32_t backend, uint32_t generation);
int32_t rsf_core_capture_vk_wants(const rsf_core_capture_vk*, uint64_t frame);
int32_t rsf_core_capture_vk_record(const rsf_core_capture_vk*, void* commands, const rsf_srvk_image*,
    const rsf_frame_record*, const char* role, uint32_t reset, uint32_t core_source, uint64_t* ticket, uint64_t group);
void rsf_core_capture_vk_complete_group(const rsf_core_capture_vk*, uint64_t group);
void rsf_core_capture_vk_discard_group(const rsf_core_capture_vk*, uint64_t group);
void rsf_core_capture_vk_retire(const rsf_core_capture_vk*, void* commands);
int32_t rsf_core_capture_vk_flush(const rsf_core_capture_vk*, uint32_t ready_only, void* commands);
}

namespace rsf {
// Serial recording only. The owner drains all referencing GPU submissions before
// flush/destruction. Buffers outlive command recording, submission and execution.
class FrameCaptureVk {
    bool command_completion = false;
    rsf_core_capture_vk* owner = nullptr;
    VkDevice device = VK_NULL_HANDLE;
    VkPhysicalDeviceMemoryProperties memory_properties{};
    PFN_vkCreateBuffer create_buffer = nullptr;
    PFN_vkDestroyBuffer destroy_buffer = nullptr;
    PFN_vkGetBufferMemoryRequirements requirements = nullptr;
    PFN_vkAllocateMemory allocate = nullptr;
    PFN_vkFreeMemory free_memory = nullptr;
    PFN_vkBindBufferMemory bind = nullptr;
    PFN_vkMapMemory map = nullptr;
    PFN_vkUnmapMemory unmap = nullptr;
    PFN_vkCmdPipelineBarrier barrier = nullptr;
    PFN_vkCmdCopyImageToBuffer copy = nullptr;
    PFN_vkCreateEvent create_event = nullptr;
    PFN_vkDestroyEvent destroy_event = nullptr;
    PFN_vkCmdSetEvent set_event = nullptr;
    PFN_vkGetEventStatus event_status = nullptr;

    template <class T> static uint64_t raw(T handle) noexcept { return reinterpret_cast<uint64_t>(handle); }
    template <class T> static T typed(uint64_t handle) noexcept { return reinterpret_cast<T>(handle); }
    static FrameCaptureVk& self(void* native) noexcept { return *static_cast<FrameCaptureVk*>(native); }

    static int32_t allocate_leaf(void* native, const rsf_srvk_image*, uint64_t bytes, uint32_t with_event,
        rsf_core_capture_vk_allocation* out) noexcept {
        auto& vk = self(native);
        VkBufferCreateInfo info{}; info.sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO;
        info.size = bytes; info.usage = VK_BUFFER_USAGE_TRANSFER_DST_BIT;
        VkBuffer buffer = VK_NULL_HANDLE;
        if (vk.create_buffer(vk.device, &info, nullptr, &buffer) != VK_SUCCESS) return -1;
        out->buffer = raw(buffer);
        VkMemoryRequirements required{}; vk.requirements(vk.device, buffer, &required);
        uint32_t type = UINT32_MAX;
        constexpr auto flags = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
        for (uint32_t i = 0; i < vk.memory_properties.memoryTypeCount; ++i) {
            if ((required.memoryTypeBits & (1u << i)) && (vk.memory_properties.memoryTypes[i].propertyFlags & flags) == flags) { type = i; break; }
        }
        if (type == UINT32_MAX) return -1;
        VkMemoryAllocateInfo allocation{}; allocation.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO;
        allocation.allocationSize = required.size; allocation.memoryTypeIndex = type;
        VkDeviceMemory memory = VK_NULL_HANDLE;
        if (vk.allocate(vk.device, &allocation, nullptr, &memory) != VK_SUCCESS) return -1;
        out->memory = raw(memory);
        if (vk.bind(vk.device, buffer, memory, 0) != VK_SUCCESS) return -1;
        if (with_event) {
            VkEventCreateInfo event_info{}; event_info.sType = VK_STRUCTURE_TYPE_EVENT_CREATE_INFO;
            VkEvent event = VK_NULL_HANDLE;
            if (vk.create_event(vk.device, &event_info, nullptr, &event) != VK_SUCCESS) return -1;
            out->event = raw(event);
        }
        return 0;
    }
    static void encode_leaf(void* native, void* raw_commands, const rsf_srvk_image* source,
        const rsf_core_capture_vk_allocation* allocation) noexcept {
        auto& vk = self(native);
        const auto commands = static_cast<VkCommandBuffer>(raw_commands);
        VkImageMemoryBarrier image{}; image.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
        image.oldLayout = VkImageLayout(source->layout); image.newLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL;
        image.srcAccessMask = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT; image.dstAccessMask = VK_ACCESS_TRANSFER_READ_BIT;
        image.srcQueueFamilyIndex = image.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
        image.image = reinterpret_cast<VkImage>(static_cast<uintptr_t>(source->image));
        const auto aspect = source->format == VK_FORMAT_D32_SFLOAT ? VK_IMAGE_ASPECT_DEPTH_BIT : VK_IMAGE_ASPECT_COLOR_BIT;
        image.subresourceRange = {VkImageAspectFlags(aspect), 0, 1, 0, 1};
        vk.barrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr, 0, nullptr, 1, &image);
        VkBufferImageCopy region{}; region.imageSubresource = {VkImageAspectFlags(aspect), 0, 0, 1};
        region.imageExtent = {source->width, source->height, 1};
        const auto buffer = typed<VkBuffer>(allocation->buffer);
        vk.copy(commands, image.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, buffer, 1, &region);
        std::swap(image.oldLayout, image.newLayout); image.srcAccessMask = VK_ACCESS_TRANSFER_READ_BIT;
        image.dstAccessMask = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
        VkBufferMemoryBarrier host{}; host.sType = VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER;
        host.srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT; host.dstAccessMask = VK_ACCESS_HOST_READ_BIT;
        host.srcQueueFamilyIndex = host.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
        host.buffer = buffer; host.size = VK_WHOLE_SIZE;
        vk.barrier(commands, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT | VK_PIPELINE_STAGE_HOST_BIT,
            0, 0, nullptr, 1, &host, 1, &image);
        if (allocation->event) vk.set_event(commands, typed<VkEvent>(allocation->event), VK_PIPELINE_STAGE_TRANSFER_BIT);
    }
    static int32_t event_set_leaf(void* native, uint64_t event) noexcept {
        auto& vk = self(native);
        return event && vk.event_status(vk.device, typed<VkEvent>(event)) == VK_EVENT_SET;
    }
    static int32_t map_leaf(void* native, uint64_t memory, uint64_t bytes, void** data) noexcept {
        auto& vk = self(native);
        return vk.map(vk.device, typed<VkDeviceMemory>(memory), 0, bytes, 0, data);
    }
    static void unmap_leaf(void* native, uint64_t memory) noexcept {
        auto& vk = self(native);
        vk.unmap(vk.device, typed<VkDeviceMemory>(memory));
    }
    static void release_leaf(void* native, const rsf_core_capture_vk_allocation* value) noexcept {
        auto& vk = self(native);
        if (value->event) vk.destroy_event(vk.device, typed<VkEvent>(value->event), nullptr);
        if (value->buffer) vk.destroy_buffer(vk.device, typed<VkBuffer>(value->buffer), nullptr);
        if (value->memory) vk.free_memory(vk.device, typed<VkDeviceMemory>(value->memory), nullptr);
    }
    static const rsf_core_capture_vk_platform* platform() noexcept {
        static const rsf_core_capture_vk_platform value{allocate_leaf, encode_leaf, event_set_leaf, map_leaf,
            unmap_leaf, release_leaf, rsf_input_dump_wants, rsf_input_dump_wants_frame,
            rsf_input_dump_reserve, rsf_input_dump_failed, rsf_input_dump_pixels, rsf_input_dump_gpu_ready};
        return &value;
    }
public:
    enum class Completion { shared_submission, command_retirement };
    FrameCaptureVk() noexcept : FrameCaptureVk(Completion::shared_submission) {}
    explicit FrameCaptureVk(Completion completion) noexcept
        : command_completion(completion == Completion::command_retirement),
          owner(rsf_core_capture_vk_create(command_completion, this, platform())) {}
    ~FrameCaptureVk() { rsf_core_capture_vk_destroy(owner); }
    FrameCaptureVk(const FrameCaptureVk&) = delete;
    FrameCaptureVk& operator=(const FrameCaptureVk&) = delete;
    bool enabled() const { return rsf_core_capture_vk_enabled(owner) != 0; }
    bool ready() const { return device != VK_NULL_HANDLE; }
    size_t pending_count() { return size_t(rsf_core_capture_vk_pending(owner)); }
    void set_provider(uint64_t version, uint32_t selected, bool fg = false) { rsf_core_capture_vk_provider(owner, version, selected, fg); }
    // Shared-submission owners also publish readbacks the shared completion proof certifies.
    bool wants(uint64_t frame) const { return rsf_core_capture_vk_wants(owner, frame) != 0; }
    bool initialize(VkDevice value, rsf_vk_get_proc proc, const VkPhysicalDeviceMemoryProperties& properties) {
        device = value; memory_properties = properties;
        rsf_core_capture_vk_bind(owner, nullptr);
#define RSF_CAPTURE_LOAD(member, function) member = reinterpret_cast<PFN_##function>(proc(device, #function)); if (!member) { device = VK_NULL_HANDLE; return !enabled(); }
        RSF_CAPTURE_LOAD(create_buffer, vkCreateBuffer); RSF_CAPTURE_LOAD(destroy_buffer, vkDestroyBuffer);
        RSF_CAPTURE_LOAD(requirements, vkGetBufferMemoryRequirements); RSF_CAPTURE_LOAD(allocate, vkAllocateMemory);
        RSF_CAPTURE_LOAD(free_memory, vkFreeMemory); RSF_CAPTURE_LOAD(bind, vkBindBufferMemory);
        RSF_CAPTURE_LOAD(map, vkMapMemory); RSF_CAPTURE_LOAD(unmap, vkUnmapMemory);
        RSF_CAPTURE_LOAD(barrier, vkCmdPipelineBarrier); RSF_CAPTURE_LOAD(copy, vkCmdCopyImageToBuffer);
        if (command_completion) {
            RSF_CAPTURE_LOAD(create_event, vkCreateEvent); RSF_CAPTURE_LOAD(destroy_event, vkDestroyEvent);
            RSF_CAPTURE_LOAD(set_event, vkCmdSetEvent); RSF_CAPTURE_LOAD(event_status, vkGetEventStatus);
        }
#undef RSF_CAPTURE_LOAD
        rsf_core_capture_vk_bind(owner, device);
        return true;
    }
    bool record(VkCommandBuffer commands, const rsf_srvk_image& source, const rsf_frame_record& identity,
        const char* role, uint32_t reset, bool core_source = true, uint64_t* core_ticket = nullptr,
        uint64_t submission_group = 0) noexcept {
        uint64_t ticket = 0;
        const bool recorded = rsf_core_capture_vk_record(owner, commands, &source, &identity, role, reset,
            core_source, &ticket, submission_group) != 0;
        if (core_ticket) *core_ticket = ticket;
        return recorded;
    }
    // An owner can attach exact queue-completion evidence to its recording
    // group. Keep discarded allocations alive until the owner's full drain.
    void complete_group(uint64_t group) { rsf_core_capture_vk_complete_group(owner, group); }
    void discard_group(uint64_t group) { rsf_core_capture_vk_discard_group(owner, group); }
    // Only match a retired recording when its caller has proven completion.
    // A full flush requires all recorded work to have drained.
    void retire_commands(VkCommandBuffer commands) noexcept { rsf_core_capture_vk_retire(owner, commands); }
    bool flush(bool ready_only = false, VkCommandBuffer completed_commands = VK_NULL_HANDLE) noexcept {
        return rsf_core_capture_vk_flush(owner, ready_only, completed_commands) != 0;
    }
};
}
