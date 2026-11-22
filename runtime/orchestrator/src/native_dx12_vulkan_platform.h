// SPDX-License-Identifier: MIT
#pragma once
#include "native_dx12_vulkan_interop.h"

typedef struct rsf_dx12vk_platform {
    int32_t (*adapter_luid)(const rsf_srvk_setup*, uint64_t*);
    int32_t (*open)(const rsf_srvk_setup*, void**);
    void (*close)(void*);
    uint32_t (*timeline_importable)(void*);
    int32_t (*create_timeline)(void*, uint64_t*);
    int32_t (*import_timeline)(void*, uint64_t, void*);
    void (*destroy_timeline)(void*, uint64_t);
    int32_t (*image_properties)(void*, uint32_t format, uint32_t extra_usage, uint32_t* importable,
        uint32_t* max_width, uint32_t* max_height);
    int32_t (*create_image)(void*, uint32_t format, uint32_t width, uint32_t height, uint32_t extra_usage, uint64_t*);
    void (*image_requirements)(void*, uint64_t image, uint64_t* bytes, uint32_t* types);
    int32_t (*handle_memory_types)(void*, void* handle, uint32_t*);
    uint32_t (*device_local_types)(void*);
    int32_t (*allocate_memory)(void*, uint64_t image, void* handle, uint64_t bytes, uint32_t type, uint64_t*);
    int32_t (*bind_image)(void*, uint64_t image, uint64_t memory);
    void (*destroy_image)(void*, uint64_t);
    void (*free_memory)(void*, uint64_t);
} rsf_dx12vk_platform;

#ifdef __cplusplus
extern "C" {
#endif
const rsf_dx12vk_platform* rsf_dx12vk_platform_ops();
#ifdef __cplusplus
}
#endif
