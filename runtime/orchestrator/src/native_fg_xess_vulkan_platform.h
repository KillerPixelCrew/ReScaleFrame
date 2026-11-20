// SPDX-License-Identifier: MIT
#pragma once
#include "native_fg_dx12_vulkan_platform.h"
#include "native_fg_xess_vulkan.h"
#include "../../vulkan/xess-fg/capture_api.h"

struct rsf_xessgvk_ops {
    const rsf_dx12gvk_ops* graphics;
    const rsf_dx12gvk_interop_ops* importer;
    decltype(&rsf_xessvk_interop_create) interop_create;
    decltype(&RsfXessBridgeCreate) create;
    decltype(&RsfXessBridgeStart) start;
    decltype(&RsfXessBridgeMaxGenerated) max_generated;
    decltype(&RsfXessBridgeSetGenerated) generated;
    int32_t (*enable)(rsf_xess_bridge*, uint32_t);
    decltype(&RsfXessBridgePresent) present;
    decltype(&RsfXessBridgeCaptureSource) capture_source;
    decltype(&RsfXessBridgeCapturedCallbacks) generated_callbacks;
    int32_t (*idle)(rsf_xess_bridge*);
    int32_t (*prepare_destroy)(rsf_xess_bridge*);
    int32_t (*destroy)(rsf_xess_bridge*);
    int32_t (*sleep)(rsf_xess_bridge*, uint32_t);
    int32_t (*marker)(rsf_xess_bridge*, uint32_t, int32_t);
    int32_t (*latency_options)(rsf_xess_bridge*, uint32_t, uint32_t);
};

struct rsf_xessvk_import_create {
    decltype(&RsfXessBridgeAdapterLuid) adapter_luid;
    decltype(&rsf_dx12vk_interop_create) create;
    decltype(rsf_dx12vk_interop_callbacks::create_image) create_image;
    decltype(rsf_dx12vk_interop_callbacks::release_image) release_image;
    decltype(rsf_dx12vk_interop_callbacks::create_fence) create_fence;
};

extern "C" int32_t rsf_core_xessvk_interop_create(const rsf_xessvk_import_create*, const rsf_srvk_setup*,
    rsf_xess_bridge*, uint32_t, rsf_xessvk_interop**);
extern "C" int32_t rsf_core_xessvk_interop_result(int32_t);
extern "C" int32_t rsf_core_xessgvk_create(const rsf_xessgvk_ops*, const rsf_fgvk_setup*, const rsf_xessgvk_options*,
    const rsf_dx12gvk_swapchain*, rsf_xessgvk**, uint64_t*);
extern "C" int32_t rsf_core_xessgvk_images(const rsf_xessgvk*, uint32_t*, uint64_t*);
extern "C" int32_t rsf_core_xessgvk_acquire(rsf_xessgvk*, uint64_t, uint64_t, uint64_t, uint32_t*);
extern "C" int32_t rsf_core_xessgvk_prepare(rsf_xessgvk*, void*, const rsf_fgvk_frame*);
extern "C" int32_t rsf_core_xessgvk_hudless(rsf_xessgvk*, void*, const rsf_fgvk_frame*);
extern "C" void rsf_core_xessgvk_submitted(rsf_xessgvk*, void* const*, uint32_t);
extern "C" int32_t rsf_core_xessgvk_enable(rsf_xessgvk*, uint32_t);
extern "C" int32_t rsf_core_xessgvk_enable_options(rsf_xessgvk*, uint32_t, uint32_t);
extern "C" uint32_t rsf_core_xessgvk_max_generated(const rsf_xessgvk*);
extern "C" int32_t rsf_core_xessgvk_present(rsf_xessgvk*, const rsf_dx12gvk_present_info*, const rsf_frame_record*, uint32_t);
extern "C" int32_t rsf_core_xessgvk_status(const rsf_xessgvk*, rsf_fgvk_status*);
extern "C" int32_t rsf_core_xessgvk_drain(rsf_xessgvk*);
extern "C" int32_t rsf_core_xessgvk_destroy(rsf_xessgvk*);
extern "C" int32_t rsf_core_xessgvk_sleep(const rsf_xessgvk*, uint32_t);
extern "C" int32_t rsf_core_xessgvk_latency(const rsf_xessgvk*, uint32_t, uint32_t);
extern "C" int32_t rsf_core_xessgvk_latency_options(const rsf_xessgvk*, uint32_t, uint32_t);
extern "C" int32_t rsf_core_xessgvk_latency_status(const rsf_xessgvk*, rsf_fgvk_latency_status*);
