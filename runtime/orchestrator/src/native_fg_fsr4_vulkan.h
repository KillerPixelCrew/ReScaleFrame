// SPDX-License-Identifier: MIT
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
#ifdef __cplusplus
}
#endif
