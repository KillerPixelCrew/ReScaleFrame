// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_fg_vulkan.h>
#include "native_dx12_vulkan_interop.h"
#include "../../vulkan/fsr4-fg/bridge_api.h"

typedef struct rsf_fsr4gvk rsf_fsr4gvk;
typedef struct rsf_fsr4gvk_options {
    uint32_t struct_size, abi_version;
    void* window;
    uint32_t timeline_enabled, generated_count;
} rsf_fsr4gvk_options;
#ifdef __cplusplus
extern "C" {
#endif
/* No HWND/WSI calls. Queries the matching DX12 device's exact major-4 provider.
   Runtime directory is setup.runtime_directory_utf8, never an Intel path. */
int32_t rsf_fsr4gvk_probe(const rsf_srvk_setup*, rsf_fsr4fg_info*);
int32_t rsf_fsr4gvk_create(const rsf_fgvk_setup*, const rsf_fsr4gvk_options*, rsf_fsr4gvk**, uint64_t*);
int32_t rsf_fsr4gvk_images(rsf_fsr4gvk*, uint32_t*, uint64_t*);
int32_t rsf_fsr4gvk_acquire(rsf_fsr4gvk*, uint64_t, uint64_t, uint64_t, uint32_t*);
int32_t rsf_fsr4gvk_prepare(rsf_fsr4gvk*, void*, const rsf_fgvk_frame*);
int32_t rsf_fsr4gvk_hudless(rsf_fsr4gvk*, void*, const rsf_fgvk_frame*);
void rsf_fsr4gvk_submitted(rsf_fsr4gvk*, void* const*, uint32_t);
int32_t rsf_fsr4gvk_present(rsf_fsr4gvk*, const void*);
int32_t rsf_fsr4gvk_enable(rsf_fsr4gvk*, uint32_t);
int32_t rsf_fsr4gvk_enable_options(rsf_fsr4gvk*, uint32_t, uint32_t);
uint32_t rsf_fsr4gvk_max_generated(const rsf_fsr4gvk*);
int32_t rsf_fsr4gvk_status(rsf_fsr4gvk*, rsf_fgvk_status*);
int32_t rsf_fsr4gvk_drain(rsf_fsr4gvk*);
int32_t rsf_fsr4gvk_destroy(rsf_fsr4gvk*);
#ifdef __cplusplus
}
#endif
