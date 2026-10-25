// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
#include <rescaleframe/native_fg_vulkan.h>
#include "../../vulkan/xess-fg/bridge_api.h"
#include "native_dx12_vulkan_interop.h"

/* Private Vulkan/DX12 resource owner. No WSI routing is installed by this API.
   The parent presenter owns the bridge, serializes calls and drains both queues
   before releasing this owner. Device creation must enable the Win32 external
   memory/semaphore extensions and timelineSemaphore before this call. */
typedef rsf_dx12vk_interop rsf_xessvk_interop;
typedef rsf_dx12vk_shared_image rsf_xessvk_shared_image;

int32_t rsf_xessvk_interop_create(const rsf_srvk_setup*, rsf_xess_bridge*,
    uint32_t timeline_enabled, rsf_xessvk_interop**);
/* Dedicated D3D12_RESOURCE import. Transfer-only optimal image, initial UNDEFINED.
   The caller records a first-use transition and explicit EXTERNAL ownership
   release/acquire around every DX12 handoff. Result lives through owner destroy. */
int32_t rsf_xessvk_interop_image(rsf_xessvk_interop*, uint32_t format,
    uint32_t width, uint32_t height, rsf_xessvk_shared_image*);
uint64_t rsf_xessvk_interop_timeline(const rsf_xessvk_interop*);
/* Require the parent to prove both Vulkan and DX12 drains. Failure retains all
   resources; this never interprets a CPU frame counter as GPU completion. */
int32_t rsf_xessvk_interop_destroy(rsf_xessvk_interop*, uint32_t queues_drained);

/* Private WSI provider. Its opaque chain is never sent to native Vulkan WSI.
   Initial implementation must be selected before any swapchain owns this HWND. */
typedef struct rsf_xessgvk rsf_xessgvk;
typedef struct rsf_xessgvk_options {
    uint32_t struct_size, abi_version;
    void* window;
    uint32_t timeline_enabled, generated_count;
    const uint16_t* intel_runtime_directory;
} rsf_xessgvk_options;
int32_t rsf_xessgvk_create(const rsf_fgvk_setup*, const rsf_xessgvk_options*, rsf_xessgvk**, uint64_t*);
int32_t rsf_xessgvk_images(rsf_xessgvk*, uint32_t*, uint64_t*);
int32_t rsf_xessgvk_acquire(rsf_xessgvk*, uint64_t, uint64_t, uint64_t, uint32_t*);
int32_t rsf_xessgvk_prepare(rsf_xessgvk*, void*, const rsf_fgvk_frame*);
int32_t rsf_xessgvk_hudless(rsf_xessgvk*, void*, const rsf_fgvk_frame*);
void rsf_xessgvk_submitted(rsf_xessgvk*, void* const*, uint32_t);
int32_t rsf_xessgvk_present(rsf_xessgvk*, const void*);
int32_t rsf_xessgvk_enable(rsf_xessgvk*, uint32_t);
int32_t rsf_xessgvk_enable_options(rsf_xessgvk*, uint32_t, uint32_t);
uint32_t rsf_xessgvk_max_generated(const rsf_xessgvk*);
int32_t rsf_xessgvk_latency(rsf_xessgvk*, uint32_t frame, uint32_t marker);
int32_t rsf_xessgvk_sleep(rsf_xessgvk*, uint32_t frame);
int32_t rsf_xessgvk_latency_options(rsf_xessgvk*, uint32_t mode, uint32_t minimum_interval_us);
int32_t rsf_xessgvk_latency_status(rsf_xessgvk*, rsf_fgvk_latency_status*);
int32_t rsf_xessgvk_status(rsf_xessgvk*, rsf_fgvk_status*);
int32_t rsf_xessgvk_drain(rsf_xessgvk*);
int32_t rsf_xessgvk_destroy(rsf_xessgvk*);
