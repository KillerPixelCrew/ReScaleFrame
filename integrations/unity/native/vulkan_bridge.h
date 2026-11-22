/* SPDX-License-Identifier: MIT */
#pragma once
/* Unity Vulkan SDK leaves in vulkan_bridge.c. interfaces is Unity's IUnityInterfaces registry,
   borrowed until plugin unload. preload installs the loader interception before device creation;
   bind configures the three plugin events from base_event. Frame routing, presentation and the
   player surface are Rust (vulkan_bridge.rs). Foreign callback guards and the Rust ingress use the
   private vulkan_foreign.h contract. */
#ifdef __cplusplus
extern "C" {
#endif
void rsf_uvk_preload(void* interfaces);
void rsf_unity_package_sdk_bind_vulkan(void* interfaces, int base_event);
void rsf_unity_package_sdk_unbind_vulkan(void);
#ifdef __cplusplus
}
#endif
