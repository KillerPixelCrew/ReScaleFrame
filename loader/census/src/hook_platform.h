/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_HOOK_PLATFORM_H
#define RSF_CENSUS_HOOK_PLATFORM_H

#include "json_platform.h"
#include <rescaleframe/vendor_census.h>

typedef struct rsf_census_hook_desc {
    const char* id;
    const char* api;
    const char* name;
    const char* modules;
} rsf_census_hook_desc;

typedef struct rsf_census_hook_platform {
    const rsf_census_hook_desc* entries;
    uint16_t count;
    uint16_t graphics_roots[4];
    int (*initialize)(void);
    void (*register_d3d)(void**);
    int (*create_api)(const uint16_t* module, const char* name, void* detour, void** original);
    int (*create)(void* target, void* detour, void** original);
    void (*remove)(void* target);
    int (*enable)(void* target);
    void (*apply)(void);
    void (*disable)(void);
    const void* (*current_teb)(void);
    bool (*read)(const void* address, void* destination, size_t bytes);
    bool (*read_name)(const void* address, char* destination, size_t capacity);
} rsf_census_hook_platform;

#endif
