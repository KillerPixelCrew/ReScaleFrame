// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_sr_vulkan.h>

#define RSF_DX12VK_INTEROP_ABI_VERSION 1u
#define RSF_DX12VK_INTEROP_INVALID_ARGUMENT (-12001)
#define RSF_DX12VK_INTEROP_ABI_MISMATCH (-12002)
typedef struct rsf_dx12vk_interop rsf_dx12vk_interop;
typedef struct rsf_dx12vk_shared_image {
    uint64_t image;
    void* dx12_resource;
    uint32_t format, width, height;
} rsf_dx12vk_shared_image;

/* Private, copied callback table. User/device and callbacks remain live through
   checked interop destruction. Returned NT handles and COM references are owned
   by this importer. Image/fence payload references survive closing NT handles. */
typedef struct rsf_dx12vk_interop_callbacks {
    uint32_t struct_size, abi_version;
    uint64_t adapter_luid;
    void* user;
    int32_t (*create_image)(void* user, uint32_t width, uint32_t height, uint32_t format,
        void** native_handle, void** resource);
    void (*release_image)(void* resource);
    int32_t (*create_fence)(void* user, void** native_handle);
} rsf_dx12vk_interop_callbacks;

#ifdef __cplusplus
extern "C" {
#endif
int32_t rsf_dx12vk_interop_create(const rsf_srvk_setup*, const rsf_dx12vk_interop_callbacks*,
    uint32_t timeline_enabled, rsf_dx12vk_interop**);
int32_t rsf_dx12vk_interop_image(rsf_dx12vk_interop*, uint32_t format,
    uint32_t width, uint32_t height, rsf_dx12vk_shared_image*);
uint64_t rsf_dx12vk_interop_timeline(const rsf_dx12vk_interop*);
/* Parent proves Vulkan AND DX12 queues drained. Failure preserves all owners. */
int32_t rsf_dx12vk_interop_destroy(rsf_dx12vk_interop*, uint32_t queues_drained);
#ifdef __cplusplus
}
#endif
