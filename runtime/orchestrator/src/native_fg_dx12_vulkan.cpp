// SPDX-License-Identifier: MIT
#include <vulkan/vulkan.h>
#include <cstddef>
#include <new>
#include "native_fg_dx12_vulkan_platform.h"
#include "../../backends/common/frame_capture_vk.h"

static_assert(sizeof(rsf_dx12gvk_ops) == 30 * sizeof(void*) &&
    offsetof(rsf_dx12gvk_ops, platform_resolve) == 3 * sizeof(void*) &&
    offsetof(rsf_dx12gvk_ops, images) == 8 * sizeof(void*) &&
    offsetof(rsf_dx12gvk_ops, describe) == 29 * sizeof(void*));
static_assert(sizeof(rsf_dx12gvk_swapchain) == 56 && offsetof(rsf_dx12gvk_swapchain, old_chain) == 48 &&
    sizeof(rsf_dx12gvk_present_info) == 32 &&
    sizeof(rsf_dx12gvk_present_submit) == 368 && offsetof(rsf_dx12gvk_present_submit, values) == 144 &&
    offsetof(rsf_dx12gvk_present_submit, signal) == 352);
static_assert(sizeof(rsf_dx12gvk_barrier) == 40 && sizeof(rsf_dx12gvk_transfer) == 32);
static_assert(sizeof(VkPresentInfoKHR) == 64 && sizeof(VkSwapchainCreateInfoKHR) == 104 &&
    sizeof(VkPhysicalDeviceMemoryProperties) == 520);

namespace {
struct CaptureWrappers {
    decltype(rsf_dx12gvk_ops::platform_allocate) allocate;
    decltype(rsf_dx12gvk_ops::platform_destroy) destroy;
    decltype(rsf_dx12gvk_ops::initialize_capture) initialize;
    decltype(rsf_dx12gvk_ops::capture_provider) provider;
    decltype(rsf_dx12gvk_ops::capture_record) record;
    decltype(rsf_dx12gvk_ops::capture_complete) complete;
    decltype(rsf_dx12gvk_ops::capture_discard) discard;
    decltype(rsf_dx12gvk_ops::capture_flush) flush;
};
extern "C" rsf_dx12gvk_platform* rsf_dx12gvk_allocate(const rsf_fgvk_setup*, rsf::FrameCaptureVk*);
extern "C" void rsf_dx12gvk_destroy(rsf_dx12gvk_platform*);
extern "C" rsf::FrameCaptureVk* rsf_dx12gvk_capture(rsf_dx12gvk_platform*);
extern "C" int32_t rsf_dx12gvk_memory_properties(rsf_dx12gvk_platform*, const rsf_srvk_setup*,
    int32_t (*)(rsf::FrameCaptureVk*, const rsf_srvk_setup*, const VkPhysicalDeviceMemoryProperties*));
extern "C" void rsf_dx12gvk_build_graphics(const CaptureWrappers*, rsf_dx12gvk_ops*);

const CaptureWrappers capture_wrappers{
    [](const auto* setup) -> rsf_dx12gvk_platform* {
        auto* capture = new (std::nothrow) rsf::FrameCaptureVk;
        if (!capture) return nullptr;
        auto* platform = rsf_dx12gvk_allocate(setup, capture);
        if (!platform) delete capture;
        return platform;
    },
    [](auto* platform) {
        delete rsf_dx12gvk_capture(platform);
        rsf_dx12gvk_destroy(platform);
    },
    [](auto* platform, const auto* graphics) -> int32_t {
        return rsf_dx12gvk_memory_properties(platform, graphics,
            [](rsf::FrameCaptureVk* capture, const rsf_srvk_setup* setup,
                const VkPhysicalDeviceMemoryProperties* memory) -> int32_t {
                return capture->initialize(static_cast<VkDevice>(setup->device), setup->device_proc, *memory)
                    ? VK_SUCCESS : VK_ERROR_INITIALIZATION_FAILED;
            });
    },
    [](auto* platform, uint64_t version, uint32_t backend) {
        rsf_dx12gvk_capture(platform)->set_provider(version, backend, true);
    },
    [](auto* platform, void* commands, const auto* source, const auto* record, const char* role, uint64_t group) {
        (void)rsf_dx12gvk_capture(platform)->record(static_cast<VkCommandBuffer>(commands), *source, *record, role,
            record->flags & RSF_FRAME_FLAG_RESET, true, nullptr, group);
    },
    [](auto* platform, uint64_t group) { rsf_dx12gvk_capture(platform)->complete_group(group); },
    [](auto* platform, uint64_t group) { rsf_dx12gvk_capture(platform)->discard_group(group); },
    [](auto* platform) -> int32_t { return rsf_dx12gvk_capture(platform)->flush() ? VK_SUCCESS : VK_ERROR_UNKNOWN; },
};
}

const rsf_dx12gvk_ops& rsf_dx12gvk_graphics() {
    static const auto ops = [] {
        rsf_dx12gvk_ops result{};
        rsf_dx12gvk_build_graphics(&capture_wrappers, &result);
        return result;
    }();
    return ops;
}
