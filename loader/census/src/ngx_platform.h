/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_NGX_PLATFORM_H
#define RSF_CENSUS_NGX_PLATFORM_H

#include "json_platform.h"

#define RSF_CENSUS_NGX_ENTRIES 42

typedef struct rsf_census_ngx_platform {
    bool (*read)(const void*, void*, size_t);
    bool (*read_string)(const void*, char*, size_t);
    bool (*read_wstring)(const void*, uint16_t*, size_t);
    bool (*parameter)(const void*, size_t slot, const char* name, void* output, int32_t* result);
    void (*d3d_resource)(rsf_census_json*, const char* key, const void* resource, bool d3d12);
    int (*decimal)(char* output, size_t capacity, double value);
} rsf_census_ngx_platform;

#endif
