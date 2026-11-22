/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_WRAPPER_PLATFORM_H
#define RSF_CENSUS_WRAPPER_PLATFORM_H

#include "json_platform.h"

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

typedef struct rsf_census_call_storage {
    uint64_t words[832];
} rsf_census_call_storage;

typedef struct rsf_census_wrapper_services {
    void* (*original)(uint16_t entry);
    rsf_census_json* (*begin)(rsf_census_call_storage*, uint16_t entry, void* caller);
    void (*finish)(rsf_census_call_storage*, uint64_t result);
    bool (*read)(const void*, void*, size_t);
    bool (*read_string)(const void*, char*, size_t);
    void* (*bind_vk)(const char*, void*);
    void (*describe_vk_queue)(void*, uint32_t family, uint32_t index);
    void (*submit_vk_queue)(void*, uint32_t count);
    void (*present_enter)(void);
    void (*present_leave)(void);
} rsf_census_wrapper_services;

#ifdef __cplusplus
extern "C" {
#endif
void rsf_census_vk_bind(const rsf_census_wrapper_services*, void** wrappers);
#ifdef __cplusplus
}
#endif
#endif
