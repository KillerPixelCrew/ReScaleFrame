// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>

struct rsf_vk_sdk_loader {
    void* (*load)(const uint16_t*);
    void (*unload)(void*);
};
static_assert(sizeof(rsf_vk_sdk_loader) == 2 * sizeof(void*));

extern "C" void* rsf_core_vk_sdk_load(const uint16_t*);
extern "C" void rsf_core_vk_sdk_unload(void*);
extern "C" uint32_t rsf_core_vk_sdk_pin(void*);
inline constexpr rsf_vk_sdk_loader rsf_vk_sdk_system_loader{rsf_core_vk_sdk_load, rsf_core_vk_sdk_unload};
