/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
#define RSF_LATENCYVK_DEVICE_ABI_VERSION 1u
typedef struct rsf_latencyvk_device_query {
    uint32_t struct_size,abi_version;
    void* instance;
    void* physical_device;
    rsf_vk_get_proc instance_proc;
    /* Caller-owned writable copy of the device feature chain. Never the engine's original. */
    void* features;
} rsf_latencyvk_device_query;
typedef struct rsf_latencyvk_device_requirements {
    uint32_t struct_size,abi_version,extension_count;
    const char* const* extensions;
    /* May include core-owned nodes, valid through the synchronous create call on this thread. */
    void* features;
} rsf_latencyvk_device_requirements;
typedef struct rsf_latencyvk_created_device {
    uint32_t struct_size,abi_version;
    void* instance;
    void* physical_device;
    void* device;
    rsf_vk_get_proc instance_proc;
    /* Borrowed actual VkDeviceCreateInfo used successfully, including any fallback. */
    const void* create_info;
} rsf_latencyvk_created_device;
#ifdef __cplusplus
extern "C" {
#endif
RSF_RUNTIME_API rsf_backend_result rsf_latencyvk_require_device(const rsf_latencyvk_device_query*,rsf_latencyvk_device_requirements*);
RSF_RUNTIME_API rsf_backend_result rsf_latencyvk_device_created(const rsf_latencyvk_created_device*);
RSF_RUNTIME_API void rsf_latencyvk_device_destroyed(void* device);
#ifdef __cplusplus
}
#endif
