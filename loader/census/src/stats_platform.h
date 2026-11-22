/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_STATS_PLATFORM_H
#define RSF_CENSUS_STATS_PLATFORM_H

#include "hook_platform.h"

typedef struct rsf_census_sample {
    uint64_t ordinal;
    uint64_t site_ordinal;
} rsf_census_sample;

typedef struct rsf_census_stats_names {
    const rsf_census_hook_desc* entries;
    const char* (*module_name)(uint32_t index);
} rsf_census_stats_names;

#ifdef __cplusplus
extern "C" {
#endif
void rsf_census_stats_present(void);
uint64_t rsf_census_stats_present_index(void);
void rsf_census_stats_queue_describe(uint8_t kind, const void* queue, uint32_t a, uint32_t b, uint32_t c);
bool rsf_census_stats_queue_submit(uint8_t kind, const void* queue, uint32_t lists);
#ifdef __cplusplus
}
#endif
#endif
