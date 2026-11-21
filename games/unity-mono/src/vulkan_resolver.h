// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>
#include <vulkan/vulkan.h>

/* Private resolver slots. Zero is the original function with no interception. */
#define RSF_UNITY_VK_FORWARD 0u
#define RSF_UNITY_VK_INSTANCE_PROC 1u
#define RSF_UNITY_VK_DEVICE_PROC 2u
#define RSF_UNITY_VK_CREATE_DEVICE 3u
#define RSF_UNITY_VK_CREATE_SWAPCHAIN 4u
#define RSF_UNITY_VK_DESTROY_SWAPCHAIN 5u
#define RSF_UNITY_VK_PRESENT 6u

extern "C" {
bool rsf_unity_vk_bind_resolver(PFN_vkVoidFunction original);
uint32_t rsf_unity_vk_resolver_route(const char* name, PFN_vkVoidFunction original, bool from_instance);
PFN_vkVoidFunction rsf_unity_vk_resolver_original(uint32_t slot);

/* Rust owns the requested extension names and the merge. Native code borrows the typed request
   for the call. A null name list preserves that request, and create reports the created device. */
int32_t rsf_unity_vk_resolver_create_device(const char* const* names, uint32_t count, void* context,
    bool (*available)(void*, const char* const*, uint32_t),
    int32_t (*create)(void*, PFN_vkVoidFunction, const char* const*, uint32_t, void**));
bool rsf_unity_vk_interop_enabled(void* device);
}
