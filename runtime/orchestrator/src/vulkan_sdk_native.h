/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct rsf_vk_sdk_loader {
    void* (*load)(const uint16_t*);
    void (*unload)(void*);
} rsf_vk_sdk_loader;

void* rsf_core_vk_sdk_load(const uint16_t*);
void rsf_core_vk_sdk_unload(void*);
uint32_t rsf_core_vk_sdk_pin(void*);

#ifdef __cplusplus
}
#endif

/* Brace initializer for the system loader table inside a constant platform table. */
#define RSF_VK_SDK_SYSTEM_LOADER {rsf_core_vk_sdk_load, rsf_core_vk_sdk_unload}
