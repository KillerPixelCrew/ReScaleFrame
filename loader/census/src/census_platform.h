/* SPDX-License-Identifier: MIT */
/* The one table of C leaves the Rust census calls. The loader's Rust archive is linked into the
   thin, shim and plugin carriers too, which carry neither MinHook nor these leaves, so Rust takes
   them from this table at install instead of naming the symbols. */
#ifndef RSF_CENSUS_PLATFORM_H
#define RSF_CENSUS_PLATFORM_H

#include "json_platform.h"
#include <rescaleframe/vendor_census.h>

#define RSF_CENSUS_NGX_ENTRIES 42

typedef struct rsf_census_hook_desc {
    const char* id;
    const char* api;
    const char* name;
    const char* modules;
} rsf_census_hook_desc;

typedef struct rsf_census_platform {
    /* RSF_CENSUS_FUNCTIONS, and the function id behind each wrapper family's local index. */
    const rsf_census_hook_desc* entries;
    uint16_t count;
    uint16_t event_entry;
    uint16_t graphics_roots[4];
    const uint16_t* d3d_entries;
    const uint16_t* vk_entries;
    const uint16_t* ngx_entries;
    const uint16_t* fsr_entries;
    const uint16_t* misc_entries;
    /* MinHook and the TEB. */
    int (*initialize)(void);
    int (*create_api)(const uint16_t* module, const char* name, void* detour, void** original);
    int (*create)(void* target, void* detour, void** original);
    void (*remove)(void* target);
    int (*enable)(void* target);
    void (*apply)(void);
    void (*disable)(void);
    const void* (*current_teb)(void);
    /* Reads of foreign memory and calls through foreign vtables, under SEH. */
    bool (*read)(const void* address, void* destination, size_t bytes);
    bool (*read_string)(const void* address, char* destination, size_t capacity);
    bool (*read_wstring)(const void* address, uint16_t* destination, size_t capacity);
    bool (*read_name)(const char* address, char* destination, size_t capacity);
    void* (*slot)(const void* object, size_t slot);
    bool (*parameter)(const void* parameters, size_t slot, const char* name, void* output, int32_t* result);
    bool (*frame_index)(const void* token, uint32_t* value);
    void (*write_ptr)(void** output, void* value);
    int32_t (*query)(void* object, const void* iid, void** result);
    void (*release)(void* object);
    bool (*queue_desc)(const void* queue, void* description);
    bool (*swapchain_desc)(const void* chain, void* description);
    bool (*resource12_desc)(const void* resource, void* description);
    bool (*resource11_dimension)(const void* resource, int32_t* dimension);
    bool (*resource11_desc)(const void* resource, void* description);
    /* printf "%.9g", the census's decimal format. */
    int (*decimal)(char* output, size_t capacity, double value);
} rsf_census_platform;

rsf_census_result rsf_census_install_impl(const rsf_census_options*, const rsf_census_platform*);

#endif