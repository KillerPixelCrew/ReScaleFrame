/* SPDX-License-Identifier: MIT */
#pragma once
#include "native_fg_xess_vulkan.h"
#include "../../vulkan/xess-fg/capture_api.h"

/* Bridge entry points handed to vulkan_xess_fg.rs. Rust selects the shared V02
   graphics table and V03 importer itself; these tables only carry the XeSS
   bridge exports, which exist only in builds with the XeSS-FG SDK. */
#ifdef __cplusplus
extern "C" {
#endif

typedef struct rsf_xessgvk_ops {
    int32_t (*interop_create)(const rsf_srvk_setup*, rsf_xess_bridge*, uint32_t, rsf_xessvk_interop**);
    int32_t (*create)(const rsf_xess_bridge_create*, rsf_xess_bridge**);
    int32_t (*start)(rsf_xess_bridge*, const rsf_xess_bridge_start*);
    uint32_t (*max_generated)(const rsf_xess_bridge*);
    int32_t (*generated)(rsf_xess_bridge*, uint32_t);
    int32_t (*enable)(rsf_xess_bridge*, uint32_t);
    int32_t (*present)(rsf_xess_bridge*, const rsf_xess_bridge_frame*, rsf_xess_bridge_present_status*);
    int32_t (*capture_source)(rsf_xess_bridge*, const rsf_frame_record*);
    uint64_t (*generated_callbacks)(const rsf_xess_bridge*);
    int32_t (*idle)(rsf_xess_bridge*);
    int32_t (*prepare_destroy)(rsf_xess_bridge*);
    int32_t (*destroy)(rsf_xess_bridge*);
    int32_t (*sleep)(rsf_xess_bridge*, uint32_t);
    int32_t (*marker)(rsf_xess_bridge*, uint32_t, int32_t);
    int32_t (*latency_options)(rsf_xess_bridge*, uint32_t, uint32_t);
} rsf_xessgvk_ops;

typedef struct rsf_xessvk_import_create {
    uint64_t (*adapter_luid)(const rsf_xess_bridge*);
    int32_t (*create)(const rsf_srvk_setup*, const rsf_dx12vk_interop_callbacks*, uint32_t, rsf_dx12vk_interop**);
    int32_t (*create_image)(void*, uint32_t, uint32_t, uint32_t, void**, void**);
    void (*release_image)(void*);
    int32_t (*create_fence)(void*, void**);
} rsf_xessvk_import_create;

_Static_assert(sizeof(rsf_xessgvk_ops) == 15 * sizeof(void*) && sizeof(rsf_xessvk_import_create) == 5 * sizeof(void*),
    "XeSS FG glue tables");
_Static_assert(sizeof(rsf_xessgvk_options) == 32 && sizeof(rsf_xess_bridge_create) == 24 &&
    sizeof(rsf_xess_bridge_start) == 32 && sizeof(rsf_xess_bridge_frame) == 216 &&
    sizeof(rsf_xess_bridge_present_status) == 24, "XeSS FG records");

/* Bridge leaves outside the public bridge_api.h, defined in bridge.c. */
int rsf_vk_xess_fg_set_enabled(rsf_xess_bridge*, uint32_t);
int rsf_vk_xess_fg_sleep(rsf_xess_bridge*, uint32_t);
int rsf_vk_xess_fg_marker(rsf_xess_bridge*, uint32_t, int);
int rsf_vk_xess_fg_set_latency_mode(rsf_xess_bridge*, uint32_t, uint32_t);
int rsf_vk_xess_fg_wait_idle(rsf_xess_bridge*);
int rsf_vk_xess_fg_prepare_destroy(rsf_xess_bridge*);
int rsf_vk_xess_fg_destroy_checked(rsf_xess_bridge*);
struct ID3D12Resource;
int rsf_vk_xess_fg_create_shared_image(rsf_xess_bridge*, uint32_t, uint32_t, uint32_t, void** shared_handle,
    struct ID3D12Resource**);
void rsf_vk_xess_fg_release_image(struct ID3D12Resource*);
int rsf_vk_xess_fg_create_shared_fence(rsf_xess_bridge*, void** shared_handle);

/* Rust owners (vulkan_xess_fg.rs, vulkan_dx12_fg/platform.rs). */
typedef struct rsf_dx12gvk_present_info rsf_dx12gvk_present_info;
typedef int32_t (*rsf_dx12gvk_present_fn)(void*, const rsf_dx12gvk_present_info*, const rsf_frame_record*, uint32_t);
int32_t rsf_dx12gvk_present_marshaled(rsf_dx12gvk_present_fn, void*, const void*, const rsf_frame_record*, uint32_t);
int32_t rsf_core_xessvk_interop_create(const rsf_xessvk_import_create*, const rsf_srvk_setup*,
    rsf_xess_bridge*, uint32_t, rsf_xessvk_interop**);
int32_t rsf_core_xessgvk_create(const rsf_xessgvk_ops*, const rsf_fgvk_setup*, const rsf_xessgvk_options*,
    rsf_xessgvk**, uint64_t*);
int32_t rsf_core_xessgvk_images(const rsf_xessgvk*, uint32_t*, uint64_t*);
int32_t rsf_core_xessgvk_acquire(rsf_xessgvk*, uint64_t, uint64_t, uint64_t, uint32_t*);
int32_t rsf_core_xessgvk_prepare(rsf_xessgvk*, void*, const rsf_fgvk_frame*);
int32_t rsf_core_xessgvk_hudless(rsf_xessgvk*, void*, const rsf_fgvk_frame*);
void rsf_core_xessgvk_submitted(rsf_xessgvk*, void* const*, uint32_t);
int32_t rsf_core_xessgvk_enable(rsf_xessgvk*, uint32_t);
int32_t rsf_core_xessgvk_enable_options(rsf_xessgvk*, uint32_t, uint32_t);
uint32_t rsf_core_xessgvk_max_generated(const rsf_xessgvk*);
int32_t rsf_core_xessgvk_present(rsf_xessgvk*, const rsf_dx12gvk_present_info*, const rsf_frame_record*, uint32_t);
int32_t rsf_core_xessgvk_status(const rsf_xessgvk*, rsf_fgvk_status*);
int32_t rsf_core_xessgvk_drain(rsf_xessgvk*);
int32_t rsf_core_xessgvk_destroy(rsf_xessgvk*);
int32_t rsf_core_xessgvk_sleep(const rsf_xessgvk*, uint32_t);
int32_t rsf_core_xessgvk_latency(const rsf_xessgvk*, uint32_t, uint32_t);
int32_t rsf_core_xessgvk_latency_options(const rsf_xessgvk*, uint32_t, uint32_t);
int32_t rsf_core_xessgvk_latency_status(const rsf_xessgvk*, rsf_fgvk_latency_status*);

#ifdef __cplusplus
}
#endif
