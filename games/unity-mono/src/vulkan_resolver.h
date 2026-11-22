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
#define RSF_UNITY_VK_CREATE_SURFACE 7u
#define RSF_UNITY_VK_DESTROY_SURFACE 8u
#define RSF_UNITY_VK_SLOTS 9u

/* Rust (vulkan_resolver.rs) owns the resolver keys and their instance and device routes, the
   registered interceptors, the saved loader functions, the interop extension selection and the
   created device's interop readiness. vulkan_bridge.cpp keeps the Vulkan types and typed calls. */
extern "C" {
/* Publishes RSF_UNITY_VK_SLOTS interceptors indexed by slot (the FORWARD entry is ignored) with
   Unity's loader resolver, and returns the resolver Unity uses: the INSTANCE_PROC interceptor, or
   original when it is null or the table is incomplete, which publishes nothing. */
PFN_vkVoidFunction rsf_unity_vk_bind_resolver(PFN_vkVoidFunction original, const PFN_vkVoidFunction* interceptors,
    uint32_t count);
/* Saves original for a routed resolver key, then returns its interceptor; otherwise returns
   original. A null original is returned and never saved. Unity lends name for the call. */
PFN_vkVoidFunction rsf_unity_vk_resolver_resolve(const char* name, PFN_vkVoidFunction original, bool from_instance);
PFN_vkVoidFunction rsf_unity_vk_resolver_original(uint32_t slot);
/* Resolves the command on the supplied device without publishing a cached continuation. */
PFN_vkVoidFunction rsf_unity_vk_resolver_device_function(uint32_t slot, void* device,
    PFN_vkVoidFunction (*resolve)(void* device, const char* name));

/* Rust owns the requested extension names, their selection and the merge. Native code borrows the
   typed request for the call; enumerate is vkEnumerateDeviceExtensionProperties on its physical
   device. A null name list preserves that request, and create reports the created device. */
int32_t rsf_unity_vk_resolver_create_device(const char* const* names, uint32_t count, void* context,
    int32_t (*enumerate)(void*, uint32_t*, VkExtensionProperties*),
    int32_t (*create)(void*, PFN_vkVoidFunction, const char* const*, uint32_t, void**));
/* Whether device was created with both interop extensions requested or, failing that, whether
   resolves (vkGetDeviceProcAddr) finds both Win32 import entry points, in order. */
bool rsf_unity_vk_interop_ready(void* device, bool (*resolves)(void* device, const char* name));
}
