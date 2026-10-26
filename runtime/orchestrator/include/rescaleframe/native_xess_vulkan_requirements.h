// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_sr_vulkan.h>

#define RSF_XESSVK_REQUIREMENTS_ABI_VERSION 1u
typedef struct rsf_xessvk_setup {
    uint32_t struct_size, abi_version;
    const char* runtime_directory_utf8;
} rsf_xessvk_setup;

#ifdef __cplusplus
extern "C" {
#endif
/* Load the private XeSS payload before Vulkan instance/device creation. Names
   and feature structures returned below are SDK-owned and valid until shutdown.
   Serialize creation calls. Never free an appended SDK feature structure. */
RSF_RUNTIME_API rsf_backend_result rsf_xessvk_initialize(const rsf_xessvk_setup*);
RSF_RUNTIME_API rsf_backend_result rsf_xessvk_instance_requirements(uint32_t* count, const char* const** names, uint32_t* minimum_api);
/* Merge names before vkCreateDevice. features addresses the application's
   writable pNext chain, including VkPhysicalDeviceFeatures2. The SDK patches
   existing structures and appends missing ones. VkDeviceCreateInfo must use
   pEnabledFeatures=nullptr. Preserve the returned chain through vkCreateDevice;
   never add duplicate feature structure types. No device is created here. */
RSF_RUNTIME_API rsf_backend_result rsf_xessvk_device_requirements(void* instance, void* physical_device,
    uint32_t* count, const char* const** names, void** features);
/* Retire SR contexts before shutdown. A live context refuses shutdown for a
   later retry. The SDK DLL stays mapped through subsequent Vulkan teardown and
   process exit, including Godot's service-before-device shutdown order. */
RSF_RUNTIME_API rsf_backend_result rsf_xessvk_shutdown(void);
#ifdef __cplusplus
}
#endif
