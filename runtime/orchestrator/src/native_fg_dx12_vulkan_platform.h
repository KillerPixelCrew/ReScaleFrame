// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_fg_vulkan.h>
#include "native_dx12_vulkan_interop.h"
#include "vulkan_fg_resources.h"
#include "vulkan_fg_copy_abi.h"

struct rsf_dx12gvk_platform;
using rsf_dx12gvk_dispatch_entries = rsf_vk_function[24];
// Raw VkSwapchainCreateInfoKHR facts. vulkan_dx12_fg.rs decides extension and present-mode policy.
// extension_count saturates at 2; extension_type is the first chained sType, or 0 without a chain.
struct rsf_dx12gvk_swapchain {
    uint32_t width, height, format, usage, color_space, layers, sharing_mode, min_images, flags;
    uint32_t present_mode, extension_count, extension_type;
    uint64_t old_chain;
};
struct rsf_dx12gvk_present_info {
    uint64_t chain;
    uint32_t index, wait_count;
    const uint64_t* waits;
    int32_t* result;
};
struct rsf_dx12gvk_present_input {
    const void* native;
    uint32_t swapchain_count, wait_count, has_extension, has_image, has_waits, has_result;
};
inline constexpr uint32_t rsf_dx12gvk_submit_wait_capacity = 17;
struct rsf_dx12gvk_present_submit {
    uint32_t wait_count;
    uint64_t waits[rsf_dx12gvk_submit_wait_capacity], values[rsf_dx12gvk_submit_wait_capacity];
    uint32_t stages[rsf_dx12gvk_submit_wait_capacity];
    uint64_t signal, ready;
};
using rsf_dx12gvk_barrier = rsf_vkfg_barrier;
// Source and target of one planned single-subresource colour copy or blit.
struct rsf_dx12gvk_transfer {
    uint64_t source, target;
    uint32_t source_width, source_height, target_width, target_height;
};
struct rsf_dx12gvk_ops {
    int32_t (*adapter_luid)(const rsf_srvk_setup*, uint64_t*);
    rsf_dx12gvk_platform* (*platform_allocate)(const rsf_fgvk_setup*);
    void (*platform_destroy)(rsf_dx12gvk_platform*);
    int32_t (*lookup)(rsf_vk_get_proc, void*, const char*, rsf_vk_function*);
    void (*platform_bind)(rsf_dx12gvk_platform*, const rsf_dx12gvk_dispatch_entries*);
    uint32_t (*format_features)(rsf_dx12gvk_platform*, uint32_t format);
    int32_t (*initialize_capture)(rsf_dx12gvk_platform*, const rsf_srvk_setup*);
    int32_t (*create_pool)(rsf_dx12gvk_platform*, uint64_t*);
    void (*destroy_pool)(rsf_dx12gvk_platform*, uint64_t);
    const rsf_vkfg_platform* images;
    int32_t (*allocate_commands)(rsf_dx12gvk_platform*, uint64_t, void**);
    int32_t (*begin)(rsf_dx12gvk_platform*, void*);
    int32_t (*end)(rsf_dx12gvk_platform*, void*);
    void (*barrier)(rsf_dx12gvk_platform*, void*, const rsf_dx12gvk_barrier*);
    void (*copy)(rsf_dx12gvk_platform*, void*, const rsf_dx12gvk_transfer*);
    void (*blit)(rsf_dx12gvk_platform*, void*, const rsf_dx12gvk_transfer*);
    void (*clear)(rsf_dx12gvk_platform*, void*, uint64_t);
    int32_t (*acquire_submit)(rsf_dx12gvk_platform*, uint64_t, uint64_t);
    int32_t (*present_submit)(rsf_dx12gvk_platform*, void*, const rsf_dx12gvk_present_submit*);
    int32_t (*wait_timeline)(rsf_dx12gvk_platform*, uint64_t, uint64_t, uint64_t);
    int32_t (*create_fence)(rsf_dx12gvk_platform*, uint64_t*);
    int32_t (*mark_submit)(rsf_dx12gvk_platform*, uint64_t);
    int32_t (*wait_fence)(rsf_dx12gvk_platform*, uint64_t, uint64_t);
    void (*destroy_fence)(rsf_dx12gvk_platform*, uint64_t);
    int32_t (*idle)(rsf_dx12gvk_platform*);
    void (*capture_provider)(rsf_dx12gvk_platform*, uint64_t, uint32_t);
    void (*capture_record)(rsf_dx12gvk_platform*, void*, const rsf_srvk_image*, const rsf_frame_record*, const char*, uint64_t);
    void (*capture_complete)(rsf_dx12gvk_platform*, uint64_t);
    void (*capture_discard)(rsf_dx12gvk_platform*, uint64_t);
    int32_t (*capture_flush)(rsf_dx12gvk_platform*);
    // Called by Rust after it admits the setup and options and checks create_info.
    void (*describe)(const void* create_info, rsf_dx12gvk_swapchain*);
};

struct rsf_dx12gvk_interop_ops {
    decltype(&rsf_dx12vk_interop_image) image;
    decltype(&rsf_dx12vk_interop_timeline) timeline;
    decltype(&rsf_dx12vk_interop_destroy) destroy;
};
constexpr rsf_dx12gvk_interop_ops rsf_dx12gvk_interop{
    rsf_dx12vk_interop_image, rsf_dx12vk_interop_timeline, rsf_dx12vk_interop_destroy,
};
extern const rsf_dx12gvk_ops rsf_dx12gvk_graphics;

using rsf_dx12gvk_present_fn = int32_t (*)(void*, const rsf_dx12gvk_present_info*, const rsf_frame_record*, uint32_t);
// Receives the admitted wait count. The caller lends output storage for that many handles.
using rsf_dx12gvk_read_present_fn = void (*)(const void*, uint32_t, uint64_t*, uint32_t*, uint64_t*);
extern "C" int32_t rsf_core_dx12gvk_present_marshaled(rsf_dx12gvk_present_fn, void*,
    const rsf_dx12gvk_present_input*, rsf_dx12gvk_read_present_fn, const rsf_frame_record*, uint32_t, int32_t*);
int32_t rsf_dx12gvk_present_marshaled(rsf_dx12gvk_present_fn, void*, const void*, const rsf_frame_record*, uint32_t);
