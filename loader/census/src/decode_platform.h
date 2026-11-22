/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_DECODE_PLATFORM_H
#define RSF_CENSUS_DECODE_PLATFORM_H

#include "json_platform.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct rsf_census_resource_reads {
    bool (*d3d12_description)(const void*, void* output);
    bool (*d3d11_dimension)(const void*, int32_t* output);
    bool (*d3d11_texture_description)(const void*, void* output);
} rsf_census_resource_reads;

void rsf_census_json_resource(rsf_census_json*, const char* key, const void* resource,
                              bool d3d12, const rsf_census_resource_reads*);

#ifdef __cplusplus
}
#endif
#endif
