/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_WRAPPER_PLATFORM_H
#define RSF_CENSUS_WRAPPER_PLATFORM_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define RSF_CENSUS_VK_ENTRIES(X) \
    X(VkGetInstanceProcAddr) X(VkGetDeviceProcAddr) X(VkCreateInstance) X(VkCreateDevice) \
    X(VkGetDeviceQueue) X(VkGetDeviceQueue2) X(VkQueueSubmit) X(VkQueueSubmit2) \
    X(VkQueueSubmit2KHR) X(VkQueuePresentKHR) X(VkCreateSwapchainKHR)

typedef enum rsf_census_vk_entry {
#define RSF_CENSUS_VK_VALUE(name) RSF_CENSUS_##name,
    RSF_CENSUS_VK_ENTRIES(RSF_CENSUS_VK_VALUE)
#undef RSF_CENSUS_VK_VALUE
    RSF_CENSUS_VK_COUNT
} rsf_census_vk_entry;

#endif
