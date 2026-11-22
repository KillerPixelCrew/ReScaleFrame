// SPDX-License-Identifier: MIT
#define VK_USE_PLATFORM_WIN32_KHR
#include <windows.h>
#include <vulkan/vulkan.h>
#include <memory>
#include <new>
#include "native_fg_dx12_vulkan_platform.h"
#include "native_sr_fsr4_vulkan_platform.h"
#include "../../backends/common/frame_capture_vk.h"
#include "sharpening_vulkan.h"

struct ID3D12Resource;
extern "C" int VulkanStoryFsr4CreateSharedImage(rsf_fsr4_bridge*, uint32_t, uint32_t,
    uint32_t, uint32_t, HANDLE*, ID3D12Resource**);
extern "C" void VulkanStoryFsr4ReleaseImage(ID3D12Resource*);
extern "C" int VulkanStoryFsr4CreateSharedFence(rsf_fsr4_bridge*, HANDLE*);
extern "C" int VulkanStoryFsr4PrepareDestroy(rsf_fsr4_bridge*);
extern "C" int VulkanStoryFsr4DestroyChecked(rsf_fsr4_bridge*);

static_assert(sizeof(rsf_fsr4_bridge_create) == 56 && offsetof(rsf_fsr4_bridge_create, output_width) == 32);
static_assert(sizeof(rsf_fsr4_bridge_frame) == 112 && offsetof(rsf_fsr4_bridge_frame, reset) == 92 &&
    offsetof(rsf_fsr4_bridge_frame, done_value) == 104);
static_assert(sizeof(rsf_fsr4_bridge_info) == sizeof(rsf_srvk_fsr4_provider_info) &&
    offsetof(rsf_fsr4_bridge_info, provider_version) == offsetof(rsf_srvk_fsr4_provider_info, provider_version));
static_assert(VK_FORMAT_R16G16B16A16_SFLOAT == 97 && VK_FORMAT_R32_SFLOAT == 100 && VK_FORMAT_R16G16_SFLOAT == 83);
static_assert(RSF_FSR4_BRIDGE_ABI_VERSION == 1 && VK_NOT_READY == 1 &&
    VK_ERROR_OUT_OF_HOST_MEMORY == -1 && VK_ERROR_INITIALIZATION_FAILED == -3 && VK_ERROR_UNKNOWN == -13);
static_assert(VK_IMAGE_LAYOUT_UNDEFINED == 0 && VK_IMAGE_LAYOUT_GENERAL == 1 &&
    VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL == 5 && VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL == 6 &&
    VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL == 7 && VK_IMAGE_USAGE_SAMPLED_BIT == 4 && VK_IMAGE_USAGE_STORAGE_BIT == 8);

namespace {
struct Graphics {
    VkDevice device{};
    PFN_vkGetPhysicalDeviceMemoryProperties memory_properties{};
    PFN_vkCmdPipelineBarrier barrier{};
    PFN_vkCmdCopyImage copy{};
    PFN_vkWaitSemaphores wait{};
    PFN_vkDeviceWaitIdle idle{};
    rsf::sharpening::Vulkan sharpen;
    rsf::FrameCaptureVk capture;
};

int32_t open(const rsf_srvk_setup* setup, void** out) noexcept try {
    auto native = std::unique_ptr<Graphics>(new (std::nothrow) Graphics{});
    if (!native) return VK_ERROR_OUT_OF_HOST_MEMORY;
    native->device = static_cast<VkDevice>(setup->device);
    native->memory_properties = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(
        setup->instance_proc(setup->instance, "vkGetPhysicalDeviceMemoryProperties"));
#define DEVICE(field, name) native->field = reinterpret_cast<PFN_##name>(setup->device_proc(setup->device, #name))
    DEVICE(barrier, vkCmdPipelineBarrier);
    DEVICE(copy, vkCmdCopyImage);
    DEVICE(wait, vkWaitSemaphores);
    DEVICE(idle, vkDeviceWaitIdle);
#undef DEVICE
    if (!native->memory_properties || !native->barrier || !native->copy || !native->wait || !native->idle)
        return VK_ERROR_EXTENSION_NOT_PRESENT;
    *out = native.release();
    return VK_SUCCESS;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

void close(void* graphics) { delete static_cast<Graphics*>(graphics); }

template<auto Call, class... Args>
int32_t checked(Args... args) noexcept try {
    return Call(args...);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

int32_t create_fence(void* bridge, void** handle) noexcept {
    return checked<VulkanStoryFsr4CreateSharedFence>(static_cast<rsf_fsr4_bridge*>(bridge), handle);
}

int32_t create_image(rsf_fsr4_bridge* bridge, uint32_t width, uint32_t height, uint32_t format,
    uint32_t writable, void** handle, void** resource) noexcept {
    ID3D12Resource* image{};
    const int32_t result = checked<VulkanStoryFsr4CreateSharedImage>(bridge, width, height, format, writable, handle, &image);
    *resource = image;
    return result;
}

void release_image(void* resource) { VulkanStoryFsr4ReleaseImage(static_cast<ID3D12Resource*>(resource)); }

int32_t capture_initialize(void* graphics, const rsf_srvk_setup* setup, uint64_t provider) noexcept try {
    auto& native = *static_cast<Graphics*>(graphics);
    VkPhysicalDeviceMemoryProperties memory{};
    native.memory_properties(static_cast<VkPhysicalDevice>(setup->physical_device), &memory);
    native.capture.set_provider(provider, RSF_SR_FSR4);
    return native.capture.initialize(native.device, setup->device_proc, memory) ? VK_SUCCESS : VK_ERROR_INITIALIZATION_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

void capture_begin(const rsf_frame_record* record) { (void)rsf_input_dump_begin_record(record, RSF_SR_FSR4); }

void capture_record(void* graphics, void* commands, const rsf_srvk_image* image, const rsf_frame_record* record,
    const char* role, uint32_t reset) {
    (void)static_cast<Graphics*>(graphics)->capture.record(static_cast<VkCommandBuffer>(commands), *image, *record, role, reset);
}

void capture_parameters(const rsf_srvk_frame* frame, uint32_t reset) {
    rsf::capture_sr_parameters(*frame->record, RSF_SR_FSR4, frame->motion_scale_x, frame->motion_scale_y, frame->pre_exposure, reset);
}

int32_t capture_flush(void* graphics) { return static_cast<Graphics*>(graphics)->capture.flush() ? 1 : 0; }

void transition(void* graphics, void* commands, const rsf_dx12gvk_barrier* plan) {
    const auto& native = *static_cast<const Graphics*>(graphics);
    VkImageMemoryBarrier barrier{VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER};
    barrier.srcAccessMask = plan->source_access;
    barrier.dstAccessMask = plan->target_access;
    barrier.oldLayout = static_cast<VkImageLayout>(plan->old_layout);
    barrier.newLayout = static_cast<VkImageLayout>(plan->new_layout);
    barrier.srcQueueFamilyIndex = plan->source_family;
    barrier.dstQueueFamilyIndex = plan->target_family;
    barrier.image = reinterpret_cast<VkImage>(plan->image);
    barrier.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    native.barrier(static_cast<VkCommandBuffer>(commands), plan->source_stages,
        plan->target_stages, 0, 0, nullptr, 0, nullptr, 1, &barrier);
}

void copy(void* graphics, void* commands, uint64_t src, uint64_t dst, uint32_t width, uint32_t height) {
    VkImageCopy region{};
    region.srcSubresource = region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    region.extent = {width, height, 1};
    static_cast<const Graphics*>(graphics)->copy(static_cast<VkCommandBuffer>(commands),
        reinterpret_cast<VkImage>(src), VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        reinterpret_cast<VkImage>(dst), VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
}

int32_t split(const rsf_fsr4vk_handoff* handoff, void* commands, uint64_t timeline,
    uint64_t previous, uint64_t ready, uint64_t done, void** continuation) noexcept try {
    return handoff->split_record(handoff->user, commands, timeline, previous, ready, done, continuation);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }


void sharpen(void* graphics, const rsf_srvk_setup* setup, void* commands, const rsf_srvk_frame* frame, uint64_t image) noexcept try {
    (void)static_cast<Graphics*>(graphics)->sharpen.run(*setup, static_cast<VkCommandBuffer>(commands), *frame, reinterpret_cast<VkImage>(image));
} catch (...) {
    // A diagnostic callback must not interrupt the shared output handoff.
}

void retire(void* graphics, void* commands) { static_cast<Graphics*>(graphics)->sharpen.retire(commands); }
void clear_sharpen(void* graphics) { static_cast<Graphics*>(graphics)->sharpen.clear(); }

int32_t wait(void* graphics, uint64_t timeline, uint64_t value, uint64_t timeout) {
    const auto& native = *static_cast<const Graphics*>(graphics);
    const auto semaphore = reinterpret_cast<VkSemaphore>(timeline);
    VkSemaphoreWaitInfo info{VK_STRUCTURE_TYPE_SEMAPHORE_WAIT_INFO};
    info.semaphoreCount = 1;
    info.pSemaphores = &semaphore;
    info.pValues = &value;
    return native.wait(native.device, &info, timeout);
}

int32_t idle(void* graphics) {
    const auto& native = *static_cast<const Graphics*>(graphics);
    return native.idle(native.device);
}

const rsf_fsr4vk_platform& platform() {
    static const rsf_fsr4vk_platform ops{
        rsf_dx12vk_platform_ops(), open, close, checked<RsfFsr4BridgeCreate>, checked<RsfFsr4BridgePlan>,
        checked<RsfFsr4BridgeInfo>, create_fence, create_image, release_image, capture_initialize, capture_begin,
        capture_record, capture_parameters, capture_flush, transition, copy, split, nullptr,
        checked<RsfFsr4BridgeSharpness>, checked<RsfFsr4BridgeEvaluate>, sharpen, retire, clear_sharpen, wait, idle,
        checked<VulkanStoryFsr4PrepareDestroy>, checked<VulkanStoryFsr4DestroyChecked>,
    };
    return ops;
}
}

int32_t rsf_fsr4vk_create(const rsf_srvk_setup* setup, const rsf_fsr4vk_handoff* handoff, rsf_fsr4vk** out) {
    return rsf_core_fsr4vk_create(&platform(), setup, handoff, out);
}
