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
bool rsf_census_stats_init(uint16_t entries, uint32_t cap, uint32_t interval);
bool rsf_census_stats_sample(uint16_t fn, uint32_t module, uint32_t rva, uint32_t tid, rsf_census_sample*);
void rsf_census_stats_counts(uint64_t* calls, uint64_t* present, uint32_t* sites, uint32_t* site_overflow);
void rsf_census_stats_present(void);
uint64_t rsf_census_stats_present_index(void);
void rsf_census_stats_queue_describe(uint8_t kind, const void* queue, uint32_t a, uint32_t b, uint32_t c);
bool rsf_census_stats_queue_submit(uint8_t kind, const void* queue, uint32_t lists);
void rsf_census_stats_sites(rsf_census_json*, const rsf_census_stats_names*);
void rsf_census_stats_histograms(rsf_census_json*, const rsf_census_hook_desc* entries);
void rsf_census_stats_queues(rsf_census_json*);
#ifdef __cplusplus
}
#endif
#endif
