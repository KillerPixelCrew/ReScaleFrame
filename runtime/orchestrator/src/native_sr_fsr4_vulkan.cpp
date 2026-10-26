// SPDX-License-Identifier: GPL-3.0-only
#define VK_USE_PLATFORM_WIN32_KHR
#include <windows.h>
#include <vulkan/vulkan.h>
#include <array>
#include <cmath>
#include <cstring>
#include <new>
#include <string>
#include "native_sr_fsr4_vulkan.h"
#include "../../backends/common/frame_capture_vk.h"

struct ID3D12Resource;
extern "C" int VulkanStoryFsr4CreateSharedImage(rsf_fsr4_bridge*, uint32_t, uint32_t,
    uint32_t, uint32_t, HANDLE*, ID3D12Resource**);
extern "C" void VulkanStoryFsr4ReleaseImage(ID3D12Resource*);
extern "C" int VulkanStoryFsr4CreateSharedFence(rsf_fsr4_bridge*, HANDLE*);
extern "C" int VulkanStoryFsr4WaitIdle(rsf_fsr4_bridge*);
extern "C" int VulkanStoryFsr4PrepareDestroy(rsf_fsr4_bridge*);
extern "C" int VulkanStoryFsr4DestroyChecked(rsf_fsr4_bridge*);

struct rsf_fsr4vk {
    rsf_srvk_setup setup{};
    rsf_fsr4vk_handoff handoff{};
    rsf_fsr4_bridge* bridge{};
    VkDevice device{};
    VkPhysicalDevice physical{};
    VkSemaphore timeline{};
    struct Image { VkImage image{}; VkDeviceMemory memory{}; ID3D12Resource* dx12{}; uint32_t width{}, height{}; };
    std::array<Image, 4> images{}; // color, depth, motion, output
    uint32_t width{}, height{};
    uint64_t last_frame{}, last_done{}, session{};
    bool initialized{}, terminal{};
    rsf::FrameCaptureVk capture;
    PFN_vkGetPhysicalDeviceImageFormatProperties2 image_properties{};
    PFN_vkGetPhysicalDeviceMemoryProperties memory_properties{};
    PFN_vkCreateImage create_image{};
    PFN_vkDestroyImage destroy_image{};
    PFN_vkGetImageMemoryRequirements requirements{};
    PFN_vkGetMemoryWin32HandlePropertiesKHR handle_properties{};
    PFN_vkAllocateMemory allocate{};
    PFN_vkFreeMemory free_memory{};
    PFN_vkBindImageMemory bind{};
    PFN_vkCreateSemaphore create_semaphore{};
    PFN_vkDestroySemaphore destroy_semaphore{};
    PFN_vkImportSemaphoreWin32HandleKHR import_semaphore{};
    PFN_vkCmdPipelineBarrier barrier{};
    PFN_vkCmdCopyImage copy{};
    PFN_vkWaitSemaphores wait{};
    PFN_vkDeviceWaitIdle idle{};
};

namespace {
int create_image(rsf_fsr4vk& self, rsf_fsr4vk::Image& slot, uint32_t format, uint32_t width, uint32_t height, bool writable) {
    VkPhysicalDeviceExternalImageFormatInfo external_query{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_EXTERNAL_IMAGE_FORMAT_INFO};
    external_query.handleType = VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT;
    VkPhysicalDeviceImageFormatInfo2 query{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_IMAGE_FORMAT_INFO_2};
    query.pNext = &external_query; query.format = static_cast<VkFormat>(format); query.type = VK_IMAGE_TYPE_2D;
    query.tiling = VK_IMAGE_TILING_OPTIMAL;
    query.usage = VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT | (writable ? VK_IMAGE_USAGE_STORAGE_BIT : 0u);
    VkExternalImageFormatProperties external{VK_STRUCTURE_TYPE_EXTERNAL_IMAGE_FORMAT_PROPERTIES};
    VkImageFormatProperties2 properties{VK_STRUCTURE_TYPE_IMAGE_FORMAT_PROPERTIES_2}; properties.pNext = &external;
    VkResult result = self.image_properties(self.physical, &query, &properties);
    if (result) return result;
    if (!(external.externalMemoryProperties.externalMemoryFeatures & VK_EXTERNAL_MEMORY_FEATURE_IMPORTABLE_BIT) ||
        width > properties.imageFormatProperties.maxExtent.width || height > properties.imageFormatProperties.maxExtent.height)
        return VK_ERROR_FORMAT_NOT_SUPPORTED;
    HANDLE handle = nullptr;
    int native_result = VulkanStoryFsr4CreateSharedImage(self.bridge, width, height, format, writable ? 1u : 0u, &handle, &slot.dx12);
    if (native_result) return native_result;
    VkExternalMemoryImageCreateInfo external_create{VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_IMAGE_CREATE_INFO};
    external_create.handleTypes = VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT;
    VkImageCreateInfo create{VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO}; create.pNext = &external_create;
    create.imageType = query.type; create.format = query.format; create.extent = {width, height, 1};
    create.mipLevels = create.arrayLayers = 1; create.samples = VK_SAMPLE_COUNT_1_BIT;
    create.tiling = query.tiling; create.usage = query.usage;
    result = self.create_image(self.device, &create, nullptr, &slot.image);
    VkMemoryRequirements requirements{};
    VkMemoryWin32HandlePropertiesKHR native{VK_STRUCTURE_TYPE_MEMORY_WIN32_HANDLE_PROPERTIES_KHR};
    if (!result) {
        self.requirements(self.device, slot.image, &requirements);
        result = self.handle_properties(self.device, VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT, handle, &native);
    }
    if (!result) {
        VkPhysicalDeviceMemoryProperties memory{}; self.memory_properties(self.physical, &memory);
        uint32_t index = memory.memoryTypeCount;
        for (uint32_t i = 0; i < memory.memoryTypeCount; ++i)
            if ((requirements.memoryTypeBits & native.memoryTypeBits & (1u << i)) &&
                (memory.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT)) { index = i; break; }
        if (index == memory.memoryTypeCount) result = VK_ERROR_INCOMPATIBLE_DRIVER;
        else {
            VkMemoryDedicatedAllocateInfo dedicated{VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO}; dedicated.image = slot.image;
            VkImportMemoryWin32HandleInfoKHR import{VK_STRUCTURE_TYPE_IMPORT_MEMORY_WIN32_HANDLE_INFO_KHR};
            import.pNext = &dedicated; import.handleType = VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT; import.handle = handle;
            VkMemoryAllocateInfo allocate{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO}; allocate.pNext = &import;
            allocate.allocationSize = requirements.size; allocate.memoryTypeIndex = index;
            result = self.allocate(self.device, &allocate, nullptr, &slot.memory);
            if (!result) result = self.bind(self.device, slot.image, slot.memory, 0);
        }
    }
    CloseHandle(handle);
    slot.width = width; slot.height = height;
    return result;
}
int wait_done(rsf_fsr4vk& self) {
    if (!self.last_done) return 0;
    VkSemaphoreWaitInfo wait{VK_STRUCTURE_TYPE_SEMAPHORE_WAIT_INFO};
    wait.semaphoreCount = 1; wait.pSemaphores = &self.timeline; wait.pValues = &self.last_done;
    return self.wait(self.device, &wait, 10000000000ull);
}
void transition(rsf_fsr4vk& self, VkCommandBuffer commands, VkImage image, VkImageLayout before,
    VkImageLayout after, bool acquire_external = false, bool release_external = false) {
    VkImageMemoryBarrier barrier{VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER};
    barrier.srcAccessMask = before == VK_IMAGE_LAYOUT_UNDEFINED || acquire_external ? 0 : VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    barrier.dstAccessMask = release_external ? 0 : VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    barrier.oldLayout = before; barrier.newLayout = after;
    // VK_QUEUE_FAMILY_EXTERNAL transfers require the actual owning family. The
    // host supplies it through the handoff, independently of SR ABI 1.
    barrier.srcQueueFamilyIndex = acquire_external ? VK_QUEUE_FAMILY_EXTERNAL : self.handoff.queue_family;
    barrier.dstQueueFamilyIndex = release_external ? VK_QUEUE_FAMILY_EXTERNAL : self.handoff.queue_family;
    if (!acquire_external && !release_external) barrier.srcQueueFamilyIndex = barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.image = image; barrier.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    self.barrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT,
        0, 0, nullptr, 0, nullptr, 1, &barrier);
}
bool input_valid(const rsf_fsr4vk& self, const rsf_srvk_image& input, uint32_t format) {
    return input.device == self.setup.device && input.image && input.copy_source && input.format == format &&
        input.width == self.width && input.height == self.height && input.layout == VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
}
}

int32_t rsf_fsr4vk_create(const rsf_srvk_setup* setup, const rsf_fsr4vk_handoff* handoff, rsf_fsr4vk** output) {
    if (!output) return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    *output = nullptr;
    if (!setup || setup->struct_size < sizeof(*setup) || !handoff || handoff->struct_size < sizeof(*handoff)) return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_SRVK_ABI_VERSION || handoff->abi_version != 1) return RSF_FSR4_BRIDGE_ABI_MISMATCH;
    if (setup->backend != 4 || setup->quality > 4 || !setup->instance || !setup->physical_device || !setup->device ||
        !setup->instance_proc || !setup->device_proc || !setup->output_width || !setup->output_height ||
        !setup->runtime_directory_utf8 || handoff->timeline_enabled != 1 || !handoff->split_record ||
        setup->depth_inverted > 1 || setup->depth_infinite > 1 || setup->motion_jittered > 1) return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    int length = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, setup->runtime_directory_utf8, -1, nullptr, 0);
    if (length <= 1) return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    std::wstring directory(static_cast<size_t>(length), L'\0');
    if (!MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, setup->runtime_directory_utf8, -1, directory.data(), length)) return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    auto* self = new (std::nothrow) rsf_fsr4vk{};
    if (!self) return VK_ERROR_OUT_OF_HOST_MEMORY;
    self->setup = *setup; self->setup.runtime_directory_utf8 = nullptr; self->handoff = *handoff;
    self->device = static_cast<VkDevice>(setup->device); self->physical = static_cast<VkPhysicalDevice>(setup->physical_device);
    auto ip = setup->instance_proc; auto dp = setup->device_proc;
#define I(field, name) self->field = reinterpret_cast<PFN_##name>(ip(setup->instance, #name))
#define D(field, name) self->field = reinterpret_cast<PFN_##name>(dp(setup->device, #name))
    I(image_properties, vkGetPhysicalDeviceImageFormatProperties2); I(memory_properties, vkGetPhysicalDeviceMemoryProperties);
    D(create_image, vkCreateImage); D(destroy_image, vkDestroyImage); D(requirements, vkGetImageMemoryRequirements);
    D(handle_properties, vkGetMemoryWin32HandlePropertiesKHR); D(allocate, vkAllocateMemory); D(free_memory, vkFreeMemory);
    D(bind, vkBindImageMemory); D(create_semaphore, vkCreateSemaphore); D(destroy_semaphore, vkDestroySemaphore);
    D(import_semaphore, vkImportSemaphoreWin32HandleKHR); D(barrier, vkCmdPipelineBarrier); D(copy, vkCmdCopyImage);
    D(wait, vkWaitSemaphores); D(idle, vkDeviceWaitIdle);
#undef I
#undef D
    auto getProperties = reinterpret_cast<PFN_vkGetPhysicalDeviceProperties2>(ip(setup->instance, "vkGetPhysicalDeviceProperties2"));
    auto semaphoreProperties = reinterpret_cast<PFN_vkGetPhysicalDeviceExternalSemaphoreProperties>(ip(setup->instance, "vkGetPhysicalDeviceExternalSemaphoreProperties"));
    if (!self->image_properties || !self->memory_properties || !self->create_image || !self->destroy_image || !self->requirements ||
        !self->handle_properties || !self->allocate || !self->free_memory || !self->bind || !self->create_semaphore ||
        !self->destroy_semaphore || !self->import_semaphore || !self->barrier || !self->copy || !self->wait || !self->idle ||
        !getProperties || !semaphoreProperties) { delete self; return VK_ERROR_EXTENSION_NOT_PRESENT; }
    VkPhysicalDeviceIDProperties identity{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES};
    VkPhysicalDeviceProperties2 properties{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2}; properties.pNext = &identity;
    getProperties(self->physical, &properties);
    uint64_t luid = 0; std::memcpy(&luid, identity.deviceLUID, sizeof(luid));
    if (!identity.deviceLUIDValid || !luid) { delete self; return VK_ERROR_INVALID_EXTERNAL_HANDLE; }
    rsf_fsr4_bridge_create create{sizeof(create), 1, luid, reinterpret_cast<const uint16_t*>(directory.c_str()),
        setup->output_width, setup->output_height, setup->output_width, setup->output_height,
        setup->depth_inverted, setup->depth_infinite, setup->motion_jittered, 0};
    int result = RsfFsr4BridgeCreate(&create, &self->bridge);
    if (!result) result = RsfFsr4BridgePlan(self->bridge, setup->quality, &self->width, &self->height);
    if (!result) {
        rsf_fsr4_bridge_info provider{sizeof(provider), RSF_FSR4_BRIDGE_ABI_VERSION};
        result = RsfFsr4BridgeInfo(self->bridge, &provider);
        if (!result) {
            VkPhysicalDeviceMemoryProperties memory{};
            self->memory_properties(self->physical, &memory);
            self->capture.set_provider(provider.provider_version, 4);
            if (!self->capture.initialize(self->device, setup->device_proc, memory)) result = VK_ERROR_INITIALIZATION_FAILED;
        }
    }
    VkSemaphoreTypeCreateInfo type{VK_STRUCTURE_TYPE_SEMAPHORE_TYPE_CREATE_INFO}; type.semaphoreType = VK_SEMAPHORE_TYPE_TIMELINE;
    VkPhysicalDeviceExternalSemaphoreInfo query{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_EXTERNAL_SEMAPHORE_INFO};
    query.pNext = &type; query.handleType = VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_D3D12_FENCE_BIT;
    VkExternalSemaphoreProperties external{VK_STRUCTURE_TYPE_EXTERNAL_SEMAPHORE_PROPERTIES};
    if (!result) {
        semaphoreProperties(self->physical, &query, &external);
        if (!(external.externalSemaphoreFeatures & VK_EXTERNAL_SEMAPHORE_FEATURE_IMPORTABLE_BIT)) result = VK_ERROR_INVALID_EXTERNAL_HANDLE;
    }
    HANDLE handle = nullptr;
    if (!result) {
        VkSemaphoreCreateInfo semaphore{VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO}; semaphore.pNext = &type;
        result = self->create_semaphore(self->device, &semaphore, nullptr, &self->timeline);
    }
    if (!result) result = VulkanStoryFsr4CreateSharedFence(self->bridge, &handle);
    if (!result) {
        VkImportSemaphoreWin32HandleInfoKHR import{VK_STRUCTURE_TYPE_IMPORT_SEMAPHORE_WIN32_HANDLE_INFO_KHR};
        import.semaphore = self->timeline; import.handleType = VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_D3D12_FENCE_BIT; import.handle = handle;
        result = self->import_semaphore(self->device, &import);
    }
    if (handle) CloseHandle(handle);
    const uint32_t formats[] = {VK_FORMAT_R16G16B16A16_SFLOAT, VK_FORMAT_R32_SFLOAT, VK_FORMAT_R16G16_SFLOAT, VK_FORMAT_R16G16B16A16_SFLOAT};
    for (uint32_t i = 0; i < self->images.size() && !result; ++i)
        result = create_image(*self, self->images[i], formats[i], i == 3 ? setup->output_width : self->width,
            i == 3 ? setup->output_height : self->height, i == 3);
    if (result) { if (rsf_fsr4vk_destroy(self)) *output = self; return result; }
    *output = self; return 0;
}

int32_t rsf_fsr4vk_plan(const rsf_fsr4vk* self, uint32_t* width, uint32_t* height) {
    if (!self || !width || !height) return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    *width = self->width; *height = self->height; return 0;
}
int32_t rsf_fsr4vk_info(const rsf_fsr4vk* self, rsf_fsr4_bridge_info* info) {
    return self ? RsfFsr4BridgeInfo(self->bridge, info) : RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
}

int32_t rsf_fsr4vk_evaluate(rsf_fsr4vk* self, void* current_commands, const rsf_srvk_frame* frame, void** continuation_commands) {
    if (!continuation_commands) return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    *continuation_commands = current_commands;
    if (!self || !current_commands || self->terminal || !frame || frame->struct_size < sizeof(*frame) || !frame->record)
        return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    if (frame->abi_version != RSF_SRVK_ABI_VERSION) return RSF_FSR4_BRIDGE_ABI_MISMATCH;
    const auto& record = *frame->record;
    if (record.struct_size < sizeof(record) || record.abi_version != RSF_GAME_FRAME_ABI_VERSION ||
        record.camera.struct_size < sizeof(record.camera) || record.camera.abi_version != RSF_GAME_FRAME_ABI_VERSION ||
        !record.frame_id || record.frame_id <= self->last_frame || !record.session_id || (self->session && self->session != record.session_id) ||
        record.render_width != self->width || record.render_height != self->height ||
        record.output_width != self->setup.output_width || record.output_height != self->setup.output_height ||
        record.camera.render_width != self->width || record.camera.render_height != self->height ||
        record.camera.output_width != record.output_width || record.camera.output_height != record.output_height ||
        record.camera.depth_inverted != self->setup.depth_inverted || record.camera.motion_jittered != self->setup.motion_jittered ||
        (record.flags & (RSF_FRAME_FLAG_NO_SR | RSF_FRAME_FLAG_AMBIGUOUS_ID)) ||
        !std::isfinite(record.frame_time_ms) || record.frame_time_ms <= 0 ||
        !std::isfinite(record.camera.near_plane) || record.camera.near_plane <= 0 ||
        !std::isfinite(record.camera.far_plane) || record.camera.far_plane < 0 ||
        (!self->setup.depth_infinite && record.camera.far_plane <= record.camera.near_plane) ||
        !std::isfinite(record.camera.vertical_fov_radians) || record.camera.vertical_fov_radians <= 0 || record.camera.vertical_fov_radians >= 3.141593f ||
        !std::isfinite(record.camera.jitter_pixels[0]) || !std::isfinite(record.camera.jitter_pixels[1]) ||
        !std::isfinite(frame->motion_scale_x) || !std::isfinite(frame->motion_scale_y) || !frame->motion_scale_x || !frame->motion_scale_y ||
        !std::isfinite(frame->pre_exposure) || frame->pre_exposure <= 0 ||
        !input_valid(*self, frame->color, VK_FORMAT_R16G16B16A16_SFLOAT) || !input_valid(*self, frame->depth, VK_FORMAT_R32_SFLOAT) ||
        !input_valid(*self, frame->motion, VK_FORMAT_R16G16_SFLOAT) || frame->output.device != self->setup.device || !frame->output.image ||
        frame->output.format != VK_FORMAT_R16G16B16A16_SFLOAT || frame->output.width != record.output_width ||
        frame->output.height != record.output_height || frame->output.layout != VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL)
        return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    (void)rsf_input_dump_begin_record(&record, 4);
    const auto commands = static_cast<VkCommandBuffer>(current_commands);
    (void)self->capture.record(commands, frame->color, record, "sr-source-jittered", record.flags & RSF_FRAME_FLAG_RESET);
    const uint32_t reset = record.flags & RSF_FRAME_FLAG_RESET;
    (void)self->capture.record(commands, frame->color, record, "sr-linear-color", reset);
    (void)self->capture.record(commands, frame->depth, record, "sr-depth", reset);
    (void)self->capture.record(commands, frame->motion, record, "sr-motion-uv", reset);
    const rsf_srvk_image* inputs[] = {&frame->color, &frame->depth, &frame->motion};
    for (uint32_t i = 0; i < 3; ++i) {
        const auto& input = *inputs[i]; const auto& target = self->images[i];
        VkImage source = reinterpret_cast<VkImage>(input.image);
        transition(*self, commands, source, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL);
        transition(*self, commands, target.image, self->initialized ? VK_IMAGE_LAYOUT_GENERAL : VK_IMAGE_LAYOUT_UNDEFINED,
            VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, self->initialized);
        VkImageCopy copy{}; copy.srcSubresource = copy.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1}; copy.extent = {self->width, self->height, 1};
        self->copy(commands, source, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, target.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &copy);
        transition(*self, commands, source, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
        transition(*self, commands, target.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_GENERAL, false, true);
    }
    if (!self->initialized) {
        transition(*self, commands, self->images[3].image, VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_GENERAL);
        transition(*self, commands, self->images[3].image, VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_LAYOUT_GENERAL, false, true);
    }
    const uint64_t ready = self->last_done + 1, done = ready + 1;
    *continuation_commands = nullptr;
    int result = self->handoff.split_record(self->handoff.user, current_commands, reinterpret_cast<uint64_t>(self->timeline),
        self->last_done, ready, done, continuation_commands);
    if (result || !*continuation_commands || *continuation_commands == current_commands) {
        self->terminal = true; return result ? result : VK_ERROR_INITIALIZATION_FAILED;
    }
    self->last_done = done; self->last_frame = record.frame_id; self->session = record.session_id; self->initialized = true;
    rsf_fsr4_bridge_frame dispatch{}; dispatch.struct_size = sizeof(dispatch); dispatch.abi_version = 1;
    dispatch.color = self->images[0].dx12; dispatch.depth = self->images[1].dx12;
    dispatch.motion = self->images[2].dx12; dispatch.output = self->images[3].dx12;
    dispatch.render_width = self->width; dispatch.render_height = self->height;
    dispatch.output_width = record.output_width; dispatch.output_height = record.output_height;
    dispatch.jitter_x = record.camera.jitter_pixels[0]; dispatch.jitter_y = record.camera.jitter_pixels[1];
    dispatch.motion_scale_x = frame->motion_scale_x; dispatch.motion_scale_y = frame->motion_scale_y;
    dispatch.delta_ms = record.frame_time_ms; dispatch.near_plane = record.camera.near_plane;
    dispatch.far_plane = record.camera.far_plane; dispatch.vertical_fov = record.camera.vertical_fov_radians;
    dispatch.pre_exposure = frame->pre_exposure; dispatch.reset = (record.flags & RSF_FRAME_FLAG_RESET) ? 1u : 0u;
    dispatch.ready_value = ready; dispatch.done_value = done;
    result = RsfFsr4BridgeEvaluate(self->bridge, &dispatch);
    if (result) return result; // Host records fallback in the returned continuation.
    const auto continuation = static_cast<VkCommandBuffer>(*continuation_commands);
    const auto& output = self->images[3];
    transition(*self, continuation, output.image, VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, true);
    VkImageCopy copy{}; copy.srcSubresource = copy.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    copy.extent = {record.output_width, record.output_height, 1};
    self->copy(continuation, output.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        reinterpret_cast<VkImage>(frame->output.image), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &copy);
    // The engine output promises only transfer destination. Capture the owned
    // source of that exact whole-image copy, whose allocation supports reads.
    const rsf_srvk_image captured_output{self->setup.device, reinterpret_cast<uint64_t>(output.image),
        VK_FORMAT_R16G16B16A16_SFLOAT, output.width, output.height, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, 1};
    rsf::capture_sr_parameters(record, RSF_SR_FSR4, frame->motion_scale_x, frame->motion_scale_y, frame->pre_exposure, dispatch.reset);
    (void)self->capture.record(continuation, captured_output, record, "sr-output-linear", dispatch.reset);
    transition(*self, continuation, output.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_GENERAL, false, true);
    return 0;
}

int32_t rsf_fsr4vk_destroy(rsf_fsr4vk* self) {
    if (!self) return 0;
    if (self->terminal) return VK_NOT_READY;
    int result = wait_done(*self);
    if (!result) result = self->idle(self->device);
    if (!result && self->bridge) result = VulkanStoryFsr4PrepareDestroy(self->bridge);
    if (!result && !self->capture.flush()) result = VK_ERROR_UNKNOWN;
    if (result) return result;
    for (auto& image : self->images) {
        if (image.image) self->destroy_image(self->device, image.image, nullptr);
        if (image.memory) self->free_memory(self->device, image.memory, nullptr);
        if (image.dx12) VulkanStoryFsr4ReleaseImage(image.dx12);
    }
    if (self->timeline) self->destroy_semaphore(self->device, self->timeline, nullptr);
    result = VulkanStoryFsr4DestroyChecked(self->bridge);
    if (result) return result;
    delete self; return 0;
}
