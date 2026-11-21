// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_fg_vulkan.h>
#include "native_dx12_vulkan_interop.h"
#include "vulkan_fg_resources.h"

struct rsf_dx12gvk_platform;
using rsf_dx12gvk_dispatch_entries = rsf_vk_function[24];
struct rsf_dx12gvk_swapchain {
    uint32_t width, height, format, usage, color_space, layers, sharing_mode, min_images, flags;
    uint32_t compatible_extensions, vsync;
    uint64_t old_chain;
};
struct rsf_dx12gvk_present_info {
    uint64_t chain;
    uint32_t index, wait_count;
    const uint64_t* waits;
    int32_t* result;
};
// One single-subresource colour image barrier planned by vulkan_dx12_fg.rs.
struct rsf_dx12gvk_barrier {
    uint64_t image;
    uint32_t old_layout, new_layout, source_family, target_family;
    uint32_t source_access, target_access, source_stages, target_stages;
};
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
    int32_t (*present_submit)(rsf_dx12gvk_platform*, void*, const rsf_dx12gvk_present_info*, uint64_t, uint64_t, uint64_t);
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

rsf_dx12gvk_swapchain rsf_dx12gvk_describe(const void* create_info);
using rsf_dx12gvk_present_fn = int32_t (*)(void*, const rsf_dx12gvk_present_info*, const rsf_frame_record*, uint32_t);
int32_t rsf_dx12gvk_present_marshaled(rsf_dx12gvk_present_fn, void*, const void*, const rsf_frame_record*, uint32_t);
