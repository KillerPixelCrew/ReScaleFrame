/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_NGX_PLATFORM_H
#define RSF_CENSUS_NGX_PLATFORM_H

#include "json_platform.h"

#define RSF_CENSUS_NGX_ENTRIES 42

typedef struct rsf_census_ngx_platform {
    bool (*read)(const void*, void*, size_t);
    bool (*read_string)(const void*, char*, size_t);
    bool (*read_wstring)(const void*, uint16_t*, size_t);
    bool (*parameter)(const void*, size_t slot, const char* name, void* output);
    void (*d3d_resource)(rsf_census_json*, const char* key, const void* resource, bool d3d12);
    void (*decimal)(rsf_census_json*, const char* key, double value);
} rsf_census_ngx_platform;

#ifdef __cplusplus
extern "C" {
#endif
void rsf_census_ngx_bind(const rsf_census_ngx_platform*, const uint16_t* entries, void** wrappers);
#ifdef __cplusplus
}
#endif
#endif
