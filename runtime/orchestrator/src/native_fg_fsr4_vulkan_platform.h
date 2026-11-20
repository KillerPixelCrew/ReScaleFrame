// SPDX-License-Identifier: MIT
#pragma once
#include "native_fg_fsr4_vulkan.h"

#include "native_fg_dx12_vulkan_platform.h"

using rsf_fsr4gvk_platform = rsf_dx12gvk_platform;
using rsf_fsr4gvk_swapchain = rsf_dx12gvk_swapchain;
using rsf_fsr4gvk_present_info = rsf_dx12gvk_present_info;
struct rsf_fsr4gvk_ops {
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
    rsf_dx12gvk_interop_ops importer;
    rsf_dx12gvk_ops graphics;
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
