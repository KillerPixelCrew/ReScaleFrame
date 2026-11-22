// SPDX-License-Identifier: MIT
#pragma once
#include "native_fg_fsr4_vulkan.h"

#include "native_fg_dx12_vulkan_platform.h"

/* Provider table consumed by vulkan_fsr4_fg.rs. Rust builds the production
   table; fixtures pass their own. Every entry is required. The bridge slots
   have the RsfFsr4Fg* signatures of bridge_api.h, spelled with the canonical
   Windows x64 types that tests/vulkan-fg/src/fsr4_state_abi.rs records. */
typedef struct rsf_fsr4gvk_ops {
    int (*probe)(unsigned long long, const unsigned short*, rsf_fsr4fg_info*);
    int (*bridge_create)(const rsf_fsr4fg_create*, rsf_fsr4fg_bridge**);
    int (*bridge_info)(const rsf_fsr4fg_bridge*, rsf_fsr4fg_info*);
    int (*bridge_start)(rsf_fsr4fg_bridge*, void*);
    int (*bridge_present)(rsf_fsr4fg_bridge*, const rsf_fsr4fg_frame*, rsf_fsr4fg_present_status*);
    int (*bridge_drain)(rsf_fsr4fg_bridge*);
    int (*bridge_prepare_destroy)(rsf_fsr4fg_bridge*);
    int (*bridge_destroy)(rsf_fsr4fg_bridge*);
    int32_t (*interop_create)(const rsf_srvk_setup*, rsf_fsr4fg_bridge*, uint64_t, uint32_t, rsf_dx12vk_interop**);
    struct rsf_dx12gvk_interop_ops importer;
    struct rsf_dx12gvk_ops graphics;
} rsf_fsr4gvk_ops;

#ifdef __cplusplus
extern "C" {
#endif
int32_t rsf_fsr4gvk_interop_create(const rsf_srvk_setup*, rsf_fsr4fg_bridge*, uint64_t, uint32_t,
    rsf_dx12vk_interop**);
int32_t rsf_core_fsr4gvk_probe(const rsf_fsr4gvk_ops*, const rsf_srvk_setup*, rsf_fsr4fg_info*);
int32_t rsf_core_fsr4gvk_create(const rsf_fsr4gvk_ops*, const rsf_fgvk_setup*, const rsf_fsr4gvk_options*,
    rsf_fsr4gvk**, uint64_t*);
int32_t rsf_core_fsr4gvk_present(rsf_fsr4gvk*, const struct rsf_dx12gvk_present_info*, const rsf_frame_record*,
    uint32_t);
#ifdef __cplusplus
}
#endif
