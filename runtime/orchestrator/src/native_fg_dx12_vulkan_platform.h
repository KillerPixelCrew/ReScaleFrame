// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_fg_vulkan.h>
#include "native_dx12_vulkan_interop.h"
#include "vulkan_fg_resources.h"

struct rsf_dx12gvk_platform;
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
struct rsf_dx12gvk_ops {
    int32_t (*adapter_luid)(const rsf_srvk_setup*, uint64_t*);
    int32_t (*platform_create)(const rsf_fgvk_setup*, uint32_t, rsf_dx12gvk_platform**);
    void (*platform_destroy)(rsf_dx12gvk_platform*);
    int32_t (*create_pool)(rsf_dx12gvk_platform*, uint64_t*);
    void (*destroy_pool)(rsf_dx12gvk_platform*, uint64_t);
    const rsf_vkfg_platform* images;
    int32_t (*allocate_commands)(rsf_dx12gvk_platform*, uint64_t, void**);
    int32_t (*begin)(rsf_dx12gvk_platform*, void*);
    int32_t (*end)(rsf_dx12gvk_platform*, void*);
    void (*copy)(rsf_dx12gvk_platform*, void*, const rsf_srvk_image*, const rsf_dx12vk_shared_image*, uint32_t);
    void (*clear)(rsf_dx12gvk_platform*, void*, const rsf_dx12vk_shared_image*);
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
