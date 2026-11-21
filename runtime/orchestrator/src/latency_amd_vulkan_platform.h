// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_latency_vulkan_requirements.h>

typedef void (*rsf_amd_vk_update)(void*, rsf_vk_function, uint32_t, uint64_t, uint32_t, uint32_t);
typedef struct rsf_amd_vk_device_info {
    const void* features;
    uint32_t extension_count;
    const char* const* extensions;
} rsf_amd_vk_device_info;

typedef struct rsf_amd_vk_platform {
    int32_t (*vendor)(const rsf_latencyvk_device_query*, uint32_t*);
    int32_t (*supported)(const rsf_latencyvk_device_query*, uint32_t*);
    void (*device_info)(const rsf_latencyvk_created_device*, rsf_amd_vk_device_info*);
    int32_t (*device_update)(const rsf_latencyvk_created_device*, rsf_vk_function*);
    rsf_amd_vk_update invoke_update;
} rsf_amd_vk_platform;

#ifdef __cplusplus
extern "C" {
#endif
int32_t rsf_core_amd_vk_require(const rsf_amd_vk_platform*, const rsf_latencyvk_device_query*,
    rsf_latencyvk_device_requirements*);
int32_t rsf_core_amd_vk_device_created(const rsf_amd_vk_platform*, const rsf_latencyvk_created_device*);
void rsf_core_amd_vk_device_destroyed(void*);
#ifdef __cplusplus
}
#endif
