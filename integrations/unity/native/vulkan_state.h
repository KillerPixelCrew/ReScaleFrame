/* SPDX-License-Identifier: MIT */
#pragma once
/* The GPU handle records the Unity package's Vulkan bridge (vulkan_state.rs, vulkan_platform.rs)
   shares with vulkan_bridge.c, which calls the Unity SDK and guards foreign callbacks. */
#include "bridge_state.h"

/* Transferred COM references and imported semaphore, plus borrowed Unity Vulkan device facts. */
typedef struct rsf_unity_vulkan_device_handles {
    void* device;
    void* queue;
    void* fence;
    uint64_t vk_device, vk_physical_device, vk_queue, vk_semaphore;
    uint32_t vk_queue_family;
    void* dispatch; /* Owned by the shared device, borrowed with this record. */
} rsf_unity_vulkan_device_handles;

/* A view borrowed under the presentation lease. The pool owns both VkCommandBuffers. */
typedef struct rsf_unity_vulkan_command_handles {
    void* allocator;
    void* commands;
    uint64_t vk[2];
    uint64_t complete;
} rsf_unity_vulkan_command_handles;

/* Rust owns these handles. */
typedef struct rsf_unity_vulkan_image_handles {
    void* resource;
    uint64_t image, memory;
} rsf_unity_vulkan_image_handles;