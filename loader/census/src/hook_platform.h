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

typedef void (*rsf_census_visit_module)(void* module, const uint16_t* path, size_t units);

typedef struct rsf_census_hook_platform {
    const rsf_census_hook_desc* entries;
    void* const* wrappers;
    uint16_t count;
    uint16_t graphics_roots[4];
    int (*create_api)(const uint16_t* module, const char* name, void* detour, void** original);
    int (*create)(void* target, void* detour, void** original);
    void (*remove)(void* target);
    int (*enable)(void* target);
    void (*apply)(void);
    void (*disable)(void);
    void (*scan_modules)(rsf_census_visit_module);
    bool (*read)(const void* address, void* destination, size_t bytes);
    bool (*read_name)(const void* address, char* destination, size_t capacity);
    bool (*active)(void);
} rsf_census_hook_platform;

#ifdef __cplusplus
extern "C" {
#endif

bool rsf_census_hooks_install(const rsf_census_hook_platform*, uint32_t flags);
void rsf_census_hooks_apply(void);
void* rsf_census_hook_original(uint16_t entry);
void* rsf_census_hook_bind_thunk(uint16_t entry, void* real);
bool rsf_census_hook_bind_inline(uint16_t entry, void* target, const char* module);
uint16_t rsf_census_hook_lookup(const char* name);

#ifdef __cplusplus
}
#endif
#endif
