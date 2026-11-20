// SPDX-License-Identifier: MIT
#pragma once
#include "native_fg_fsr4_vulkan.h"

struct rsf_fsr4gvk_platform;
struct rsf_fsr4gvk_swapchain {
    uint32_t width, height, format, usage, color_space, layers, sharing_mode, min_images, flags;
    uint32_t compatible_extensions, vsync;
    uint64_t old_chain;
};
struct rsf_fsr4gvk_present_info {
    uint64_t chain;
    uint32_t index, wait_count;
    const uint64_t* waits;
    int32_t* result;
};
struct rsf_fsr4gvk_ops {
    int32_t (*adapter_luid)(const rsf_srvk_setup*, uint64_t*);
    int32_t (*directory)(const char*, uint16_t*, int32_t);
    decltype(&RsfFsr4FgProbe) probe;
    decltype(&RsfFsr4FgCreate) bridge_create;
    decltype(&RsfFsr4FgInfo) bridge_info;
    decltype(&RsfFsr4FgStart) bridge_start;
    decltype(&RsfFsr4FgPresent) bridge_present;
    decltype(&RsfFsr4FgDrain) bridge_drain;
    decltype(&RsfFsr4FgPrepareDestroy) bridge_prepare_destroy;
    decltype(&RsfFsr4FgDestroy) bridge_destroy;
    int32_t (*interop_create)(const rsf_srvk_setup*, rsf_fsr4fg_bridge*, uint64_t, uint32_t, rsf_dx12vk_interop**);
    decltype(&rsf_dx12vk_interop_image) interop_image;
    decltype(&rsf_dx12vk_interop_timeline) interop_timeline;
    decltype(&rsf_dx12vk_interop_destroy) interop_destroy;
    int32_t (*platform_create)(const rsf_fgvk_setup*, rsf_fsr4gvk_platform**);
    void (*platform_destroy)(rsf_fsr4gvk_platform*);
    int32_t (*create_pool)(rsf_fsr4gvk_platform*, uint64_t*);
    void (*destroy_pool)(rsf_fsr4gvk_platform*, uint64_t);
    int32_t (*create_image)(rsf_fsr4gvk_platform*, const rsf_fsr4gvk_swapchain*, uint64_t*, uint64_t*);
    void (*destroy_image)(rsf_fsr4gvk_platform*, uint64_t, uint64_t);
    int32_t (*allocate_commands)(rsf_fsr4gvk_platform*, uint64_t, void**);
    int32_t (*begin)(rsf_fsr4gvk_platform*, void*);
    int32_t (*end)(rsf_fsr4gvk_platform*, void*);
    void (*copy)(rsf_fsr4gvk_platform*, void*, const rsf_srvk_image*, const rsf_dx12vk_shared_image*, uint32_t);
    void (*clear)(rsf_fsr4gvk_platform*, void*, const rsf_dx12vk_shared_image*);
    int32_t (*acquire_submit)(rsf_fsr4gvk_platform*, uint64_t, uint64_t);
    int32_t (*present_submit)(rsf_fsr4gvk_platform*, void*, const rsf_fsr4gvk_present_info*, uint64_t, uint64_t, uint64_t);
    int32_t (*wait_timeline)(rsf_fsr4gvk_platform*, uint64_t, uint64_t, uint64_t);
    int32_t (*create_fence)(rsf_fsr4gvk_platform*, uint64_t*);
    int32_t (*mark_submit)(rsf_fsr4gvk_platform*, uint64_t);
    int32_t (*wait_fence)(rsf_fsr4gvk_platform*, uint64_t, uint64_t);
    void (*destroy_fence)(rsf_fsr4gvk_platform*, uint64_t);
    int32_t (*idle)(rsf_fsr4gvk_platform*);
    void (*capture_provider)(rsf_fsr4gvk_platform*, uint64_t);
    void (*capture_record)(rsf_fsr4gvk_platform*, void*, const rsf_srvk_image*, const rsf_frame_record*, const char*, uint64_t);
    void (*capture_complete)(rsf_fsr4gvk_platform*, uint64_t);
    void (*capture_discard)(rsf_fsr4gvk_platform*, uint64_t);
    int32_t (*capture_flush)(rsf_fsr4gvk_platform*);
};

extern "C" int32_t rsf_core_fsr4gvk_probe(const rsf_fsr4gvk_ops*, const rsf_srvk_setup*, rsf_fsr4fg_info*);
extern "C" int32_t rsf_core_fsr4gvk_create(const rsf_fsr4gvk_ops*, const rsf_fgvk_setup*, const rsf_fsr4gvk_options*,
    const rsf_fsr4gvk_swapchain*, rsf_fsr4gvk**, uint64_t*);
extern "C" int32_t rsf_core_fsr4gvk_present(rsf_fsr4gvk*, const rsf_fsr4gvk_present_info*, const rsf_frame_record*, uint32_t);
extern "C" int32_t rsf_core_fsr4gvk_images(const rsf_fsr4gvk*, uint32_t*, uint64_t*);
extern "C" int32_t rsf_core_fsr4gvk_acquire(rsf_fsr4gvk*, uint64_t, uint64_t, uint64_t, uint32_t*);
extern "C" int32_t rsf_core_fsr4gvk_prepare(rsf_fsr4gvk*, void*, const rsf_fgvk_frame*);
extern "C" int32_t rsf_core_fsr4gvk_hudless(rsf_fsr4gvk*, void*, const rsf_fgvk_frame*);
extern "C" void rsf_core_fsr4gvk_submitted(rsf_fsr4gvk*, void* const*, uint32_t);
extern "C" int32_t rsf_core_fsr4gvk_enable_options(rsf_fsr4gvk*, uint32_t, uint32_t);
extern "C" uint32_t rsf_core_fsr4gvk_max_generated(const rsf_fsr4gvk*);
extern "C" int32_t rsf_core_fsr4gvk_status(const rsf_fsr4gvk*, rsf_fgvk_status*);
extern "C" int32_t rsf_core_fsr4gvk_drain(rsf_fsr4gvk*);
extern "C" int32_t rsf_core_fsr4gvk_destroy(rsf_fsr4gvk*);
