// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
#include <rescaleframe/capture_names.h>
#include <windows.h>
#include <vulkan/vulkan.h>
#include <algorithm>
#include <cwchar>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <memory>
#include <vector>
#include <mutex>

namespace rsf {
// Serial recording only. The owner drains all referencing GPU submissions before
// flush/destruction. Buffers outlive command recording, submission and execution.
class FrameCaptureVk {
    struct Readback {
        VkBuffer buffer = VK_NULL_HANDLE;
        VkDeviceMemory memory = VK_NULL_HANDLE;
        VkDeviceSize bytes = 0;
        rsf_srvk_image image{};
        rsf_frame_record identity{};
        std::string role;
        uint32_t reset = 0;
        bool recorded = false;
        bool legacy = false;
        uint64_t core_ticket = 0;
        std::string core_name;
    };
    std::filesystem::path directory;
    uint64_t first = 30, count = 4;
    uint64_t provider = 0;
    uint32_t backend = 0;
    bool generation = false;
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
    std::vector<std::unique_ptr<Readback>> pending;
    std::mutex guard;
    void release(Readback& value) {
        if (value.buffer) destroy_buffer(device, value.buffer, nullptr);
        if (value.memory) free_memory(device, value.memory, nullptr);
        value.buffer = VK_NULL_HANDLE; value.memory = VK_NULL_HANDLE;
    }
    void failure(const char* role, uint64_t frame) noexcept try {
        if (!directory.empty()) std::ofstream(directory / "capture-errors.log", std::ios::app) << frame << ' ' << role << '\n';
    } catch (...) {}
public:
    FrameCaptureVk() noexcept { try {
        wchar_t path[32768]{}, value[32]{};
        const auto size = GetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", path, 32768);
        if (!size || size >= 32768) return;
        directory = path;
        if (GetEnvironmentVariableW(L"RSF_FRAME_CAPTURE_FIRST", value, 32)) first = std::wcstoull(value, nullptr, 10);
        if (GetEnvironmentVariableW(L"RSF_FRAME_CAPTURE_COUNT", value, 32)) count = (std::min)(uint64_t(16), std::wcstoull(value, nullptr, 10));
        std::filesystem::create_directories(directory);
    } catch (...) { directory.clear(); } }
    ~FrameCaptureVk() { for (auto& value : pending) release(*value); }
    bool enabled() const { return !directory.empty() && count; }
    bool ready() const { return device != VK_NULL_HANDLE; }
    size_t pending_count() { std::lock_guard lock(guard); return pending.size(); }
    void set_provider(uint64_t version, uint32_t selected, bool fg = false) { provider = version; backend = selected; generation = fg; }
    bool wants(uint64_t frame) const {
        (void)const_cast<FrameCaptureVk*>(this)->flush(true);
        return (enabled() && frame >= first && frame - first < count) || rsf_input_dump_wants_frame(frame);
    }
    bool initialize(VkDevice value, rsf_vk_get_proc proc, const VkPhysicalDeviceMemoryProperties& properties) {
        device = value; memory_properties = properties;
#define RSF_CAPTURE_LOAD(member, function) member = reinterpret_cast<PFN_##function>(proc(device, #function)); if (!member) { device = VK_NULL_HANDLE; return !enabled(); }
        RSF_CAPTURE_LOAD(create_buffer, vkCreateBuffer); RSF_CAPTURE_LOAD(destroy_buffer, vkDestroyBuffer);
        RSF_CAPTURE_LOAD(requirements, vkGetBufferMemoryRequirements); RSF_CAPTURE_LOAD(allocate, vkAllocateMemory);
        RSF_CAPTURE_LOAD(free_memory, vkFreeMemory); RSF_CAPTURE_LOAD(bind, vkBindBufferMemory);
        RSF_CAPTURE_LOAD(map, vkMapMemory); RSF_CAPTURE_LOAD(unmap, vkUnmapMemory);
        RSF_CAPTURE_LOAD(barrier, vkCmdPipelineBarrier); RSF_CAPTURE_LOAD(copy, vkCmdCopyImageToBuffer);
#undef RSF_CAPTURE_LOAD
        return true;
    }
    bool record(VkCommandBuffer commands, const rsf_srvk_image& source, const rsf_frame_record& identity,
        const char* role, uint32_t reset, bool core_source = true, uint64_t* core_ticket = nullptr) noexcept try {
        if (core_ticket) *core_ticket = 0;
        if (!wants(identity.frame_id)) return true;
        if (!device || !commands || source.device != device || !source.width || !source.height) return false;
        std::lock_guard lock(guard);
        if (!source.copy_source) { failure(role, identity.frame_id); return false; }
        uint32_t pixel_bytes = 0;
        switch (source.format) {
        case VK_FORMAT_R16G16B16A16_SFLOAT: pixel_bytes = 8; break;
        case VK_FORMAT_R32G32_SFLOAT: pixel_bytes = 8; break;
        case VK_FORMAT_R8_UNORM: pixel_bytes = 1; break;
        case VK_FORMAT_R16G16_SFLOAT:
        case VK_FORMAT_R8G8B8A8_UNORM:
        case VK_FORMAT_R8G8B8A8_SRGB:
        case VK_FORMAT_B8G8R8A8_UNORM:
        case VK_FORMAT_B8G8R8A8_SRGB:
        case VK_FORMAT_A2B10G10R10_UNORM_PACK32:
        case VK_FORMAT_R32_SFLOAT:
        case VK_FORMAT_D32_SFLOAT: pixel_bytes = 4; break;
        default: failure(role, identity.frame_id); return false;
        }
        auto owned = std::make_unique<Readback>();
        owned->image = source; owned->identity = identity; owned->role = role; owned->reset = reset;
        owned->legacy = enabled() && identity.frame_id >= first && identity.frame_id - first < count;
        if (core_source && rsf_input_dump_wants(&identity)) {
            owned->core_name = capture_name(role);
            owned->core_ticket = rsf_input_dump_reserve(&identity, owned->core_name.c_str(), role,
                capture_view(owned->core_name), capture_scale(role));
            if (core_ticket) *core_ticket = owned->core_ticket;
        }
        if (!owned->legacy && !owned->core_ticket) return true;
        owned->bytes = VkDeviceSize(source.width) * source.height * pixel_bytes;
        if (!owned->bytes || owned->bytes > 256ull * 1024 * 1024) {
            rsf_input_dump_failed(owned->core_ticket, owned->core_name.c_str(), RSF_TEXTURE_ERROR_UNSUPPORTED_FORMAT); return false;
        }
        // Retain even a partially allocated buffer. Failure records no GPU copy;
        // successful copies keep their allocation until drained retirement.
        pending.push_back(std::move(owned)); auto& value = *pending.back();
        VkBufferCreateInfo info{}; info.sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO;
        info.size = value.bytes; info.usage = VK_BUFFER_USAGE_TRANSFER_DST_BIT;
        if (create_buffer(device, &info, nullptr, &value.buffer) != VK_SUCCESS) { failure(role, identity.frame_id); return false; }
        VkMemoryRequirements required{}; requirements(device, value.buffer, &required);
        uint32_t type = UINT32_MAX;
        constexpr auto flags = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
        for (uint32_t i = 0; i < memory_properties.memoryTypeCount; ++i) {
            if ((required.memoryTypeBits & (1u << i)) && (memory_properties.memoryTypes[i].propertyFlags & flags) == flags) { type = i; break; }
        }
        if (type == UINT32_MAX) { failure(role, identity.frame_id); return false; }
        VkMemoryAllocateInfo allocation{}; allocation.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO;
        allocation.allocationSize = required.size; allocation.memoryTypeIndex = type;
        if (allocate(device, &allocation, nullptr, &value.memory) != VK_SUCCESS || bind(device, value.buffer, value.memory, 0) != VK_SUCCESS) {
            failure(role, identity.frame_id); return false;
        }
        VkImageMemoryBarrier image{}; image.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
        image.oldLayout = VkImageLayout(source.layout); image.newLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL;
        image.srcAccessMask = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT; image.dstAccessMask = VK_ACCESS_TRANSFER_READ_BIT;
        image.srcQueueFamilyIndex = image.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
        image.image = reinterpret_cast<VkImage>(static_cast<uintptr_t>(source.image));
        const auto aspect = source.format == VK_FORMAT_D32_SFLOAT ? VK_IMAGE_ASPECT_DEPTH_BIT : VK_IMAGE_ASPECT_COLOR_BIT;
        image.subresourceRange = {VkImageAspectFlags(aspect), 0, 1, 0, 1};
        barrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr, 0, nullptr, 1, &image);
        VkBufferImageCopy region{}; region.imageSubresource = {VkImageAspectFlags(aspect), 0, 0, 1};
        region.imageExtent = {source.width, source.height, 1};
        copy(commands, image.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, value.buffer, 1, &region);
        std::swap(image.oldLayout, image.newLayout); image.srcAccessMask = VK_ACCESS_TRANSFER_READ_BIT;
        image.dstAccessMask = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
        VkBufferMemoryBarrier host{}; host.sType = VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER;
        host.srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT; host.dstAccessMask = VK_ACCESS_HOST_READ_BIT;
        host.srcQueueFamilyIndex = host.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
        host.buffer = value.buffer; host.size = VK_WHOLE_SIZE;
        barrier(commands, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT | VK_PIPELINE_STAGE_HOST_BIT,
            0, 0, nullptr, 1, &host, 1, &image);
        value.recorded = true;
        return true;
    } catch (...) { failure(role, identity.frame_id); return false; }
    // Caller has completed all command buffers containing these readbacks.
    bool flush(bool ready_only = false) noexcept try {
        std::lock_guard lock(guard);
        bool okay = true;
        for (auto it = pending.begin(); it != pending.end();) {
            auto& value = **it;
            if (ready_only && value.legacy && !value.core_ticket) { ++it; continue; }
            if (!value.recorded) { okay = false; rsf_input_dump_failed(value.core_ticket, value.core_name.c_str(), RSF_TEXTURE_ERROR_STAGING_FAILED); release(value); it = pending.erase(it); continue; }
            if (value.core_ticket && !rsf_input_dump_gpu_ready(value.core_ticket, value.core_name.c_str(), 3, device)) {
                if (ready_only) { ++it; continue; }
                okay = false; rsf_input_dump_failed(value.core_ticket, value.core_name.c_str(), RSF_TEXTURE_ERROR_MAP_FAILED);
                value.core_ticket = 0;
                if (!value.legacy) { release(value); it = pending.erase(it); continue; }
            }
            void* data = nullptr;
            if (map(device, value.memory, 0, value.bytes, 0, &data) != VK_SUCCESS) { okay = false; rsf_input_dump_failed(value.core_ticket, value.core_name.c_str(), RSF_TEXTURE_ERROR_MAP_FAILED); release(value); it = pending.erase(it); continue; }
            if (value.core_ticket) {
                uint32_t encoding = 0, pixel_size = 4;
                switch (value.image.format) {
                case VK_FORMAT_R16G16B16A16_SFLOAT: encoding = 10; pixel_size = 8; break;
                case VK_FORMAT_R32G32_SFLOAT: encoding = 16; pixel_size = 8; break;
                case VK_FORMAT_R16G16_SFLOAT: encoding = 34; break;
                case VK_FORMAT_R8G8B8A8_UNORM: encoding = 28; break;
                case VK_FORMAT_R8G8B8A8_SRGB: encoding = 29; break;
                case VK_FORMAT_B8G8R8A8_UNORM: encoding = 87; break;
                case VK_FORMAT_B8G8R8A8_SRGB: encoding = 91; break;
                case VK_FORMAT_A2B10G10R10_UNORM_PACK32: encoding = 24; break;
                case VK_FORMAT_R32_SFLOAT: encoding = 41; break;
                case VK_FORMAT_D32_SFLOAT: encoding = 40; break;
                case VK_FORMAT_R8_UNORM: encoding = 61; pixel_size = 1; break;
                default: break;
                }
                rsf_dump_pixels pixels{sizeof(pixels), value.image.width, value.image.height, encoding,
                    value.image.width * pixel_size, value.bytes, data};
                okay &= rsf_input_dump_pixels(value.core_ticket, value.core_name.c_str(), &pixels, 3, value.image.format) != 0;
                value.core_ticket = 0;
            }
            if (ready_only && value.legacy) { unmap(device, value.memory); ++it; continue; }
            if (!value.legacy) { unmap(device, value.memory); release(value); it = pending.erase(it); continue; }
            const auto stem = std::to_string(value.identity.session_id) + '-' + std::to_string(value.identity.frame_id) + '-' + value.role + "-0";
            std::ofstream raw(directory / (stem + ".raw"), std::ios::binary);
            raw.write(static_cast<const char*>(data), static_cast<std::streamsize>(value.bytes)); unmap(device, value.memory);
            std::ofstream json(directory / (stem + ".json"));
            const auto& camera = value.identity.camera;
            const auto pixel_bytes = value.image.format == VK_FORMAT_R16G16B16A16_SFLOAT ? 8 : 4;
            json << std::setprecision(9) << "{\n\"role\":\"" << value.role << "\",\n\"frame\":" << value.identity.frame_id
                << ",\n\"session\":" << value.identity.session_id << ",\n\"view\":" << value.identity.view_id
                << ",\n\"generation\":" << value.identity.resource_generation << ",\n\"flags\":" << value.identity.flags
                << ",\n\"frame_time_ms\":" << value.identity.frame_time_ms << ",\n\"width\":" << value.image.width
                << ",\n\"height\":" << value.image.height << ",\n\"row_pitch\":" << value.image.width * pixel_bytes
                << ",\n\"offset\":0,\n\"vk_format\":" << value.image.format << ",\n\"bytes\":" << value.bytes
                << ",\n\"sdk_dispatch_reset\":" << value.reset << ",\n\"completion\":\"owner-drained-context-retirement\""
                << ",\n\"" << (generation ? "fg_backend" : "sr_backend") << "\":" << backend << ",\n\"provider_version_id\":" << provider
                << ",\n\"render_size\":[" << camera.render_width << ',' << camera.render_height << "],\n\"output_size\":[" << camera.output_width << ',' << camera.output_height
                << "],\n\"jitter_pixels\":[" << camera.jitter_pixels[0] << ',' << camera.jitter_pixels[1] << "],\n\"matrix_convention\":\"row_vectors\"";
            auto matrix = [&json](const char* key, const float* values) {
                json << ",\n\"" << key << "\":[";
                for (uint32_t i = 0; i < 16; ++i) { if (i) json << ','; json << values[i]; } json << ']';
            };
            matrix("view_to_clip", camera.view_to_clip); matrix("clip_to_view", camera.clip_to_view);
            matrix("view_to_world", camera.view_to_world); matrix("world_to_view", camera.world_to_view);
            matrix("clip_to_previous_clip", camera.clip_to_previous_clip);
            json << "\n}\n"; okay &= bool(raw) && bool(json);
            release(value);
            it = pending.erase(it);
        }
        if (!okay) failure("readback-flush", 0); return okay;
    } catch (...) { return false; }
};
}
