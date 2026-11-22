/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_DECODE_PLATFORM_H
#define RSF_CENSUS_DECODE_PLATFORM_H

#include "json_platform.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct rsf_census_resource_desc {
    uint64_t width;
    uint32_t height;
    uint32_t dimension;
    uint32_t format;
    uint32_t layers;
    uint32_t mips;
    uint32_t samples;
    uint32_t flags;
    bool dimension_read;
    bool texture_read;
} rsf_census_resource_desc;

typedef bool (*rsf_census_read_resource)(const void*, rsf_census_resource_desc*);

const char* rsf_census_dxgi_format_name(uint32_t format);
void rsf_census_json_resource(rsf_census_json*, const char* key, const void* resource,
                              bool d3d12, rsf_census_read_resource read);

#ifdef __cplusplus
}
#endif
#endif
