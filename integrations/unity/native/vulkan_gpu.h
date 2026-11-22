// SPDX-License-Identifier: MIT
#pragma once

#include "vulkan_state.h"
#include <vulkan/vulkan.h>

/* Private Rust graphics calls. The Unity callback holds the presentation lease through all
   uses of the device, image and slot views. Creation transfers partial results to the state
   owner for rollback. No Unity interface, C++ exception or COM smart pointer crosses here. */
extern "C" {
/* 1 on success, 2 when Vulkan cannot identify its Windows adapter, otherwise 0. */
uint32_t rsf_uvk_gpu_create_device(rsf_unity_vulkan_device_handles*, uint64_t instance,
    PFN_vkGetInstanceProcAddr, PFN_vkGetDeviceProcAddr);
void rsf_uvk_gpu_destroy_device(const rsf_unity_vulkan_device_handles*);
uint64_t rsf_uvk_gpu_create_pool(const rsf_unity_vulkan_device_handles*);
void rsf_uvk_gpu_destroy_pool(const rsf_unity_vulkan_device_handles*, uint64_t);
bool rsf_uvk_gpu_create_slot(const rsf_unity_vulkan_device_handles*, uint64_t, rsf_unity_vulkan_command_handles*);
void rsf_uvk_gpu_destroy_slot(const rsf_unity_vulkan_device_handles*, const rsf_unity_vulkan_command_handles*);
bool rsf_uvk_gpu_create_image(const rsf_unity_vulkan_device_handles*, uint32_t, uint32_t, uint32_t, uint32_t,
    rsf_unity_vulkan_image_handles*);
void rsf_uvk_gpu_destroy_image(const rsf_unity_vulkan_device_handles*, const rsf_unity_vulkan_image_handles*);
bool rsf_uvk_gpu_fence_ready(const rsf_unity_vulkan_device_handles*, uint64_t);
uint64_t rsf_uvk_gpu_shared_completed(const rsf_unity_vulkan_device_handles*);
bool rsf_uvk_gpu_drain(const rsf_unity_vulkan_device_handles*);

bool rsf_uvk_gpu_begin(const rsf_unity_vulkan_device_handles*, const rsf_unity_vulkan_command_handles*, uint32_t ring);
bool rsf_uvk_gpu_upload(const rsf_unity_vulkan_device_handles*, const rsf_unity_vulkan_command_handles*,
    const VkSemaphore* waits, uint32_t count);
bool rsf_uvk_gpu_submit(const rsf_unity_vulkan_device_handles*, const rsf_unity_vulkan_command_handles*,
    uint32_t ring, uint64_t ready);
void rsf_uvk_gpu_image_transition(const rsf_unity_vulkan_device_handles*, uint64_t command,
    uint64_t image, uint32_t index, bool output, bool acquire);
void rsf_uvk_gpu_copy(const rsf_unity_vulkan_device_handles*, uint64_t command, uint64_t source,
    int32_t source_layout, uint32_t aspect, uint64_t target, int32_t target_layout, uint32_t width, uint32_t height);
void rsf_uvk_gpu_blit_present(const rsf_unity_vulkan_device_handles*, uint64_t command,
    uint64_t presented, uint64_t target, uint32_t width, uint32_t height);
void rsf_uvk_gpu_dx_barrier(void* commands, void* resource, int32_t before, int32_t after);
void rsf_uvk_gpu_dx_copy(void* commands, void* target, void* source);
/* The real HWND swap chain and its current back buffer transfer one reference each. */
void* rsf_uvk_gpu_chain_create(const rsf_unity_vulkan_device_handles*, void* window, uint32_t width, uint32_t height);
void rsf_uvk_gpu_chain_extent(void* chain, uint32_t* width, uint32_t* height);
bool rsf_uvk_gpu_chain_resize(void* chain, uint32_t width, uint32_t height);
void rsf_uvk_gpu_chain_destroy(void* chain);
void* rsf_uvk_gpu_chain_buffer(void* chain);
bool rsf_uvk_gpu_chain_present(void* chain);
}
