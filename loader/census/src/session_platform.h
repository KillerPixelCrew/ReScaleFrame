/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_SESSION_PLATFORM_H
#define RSF_CENSUS_SESSION_PLATFORM_H

#include "record_platform.h"
#include "census_platform.h"

typedef struct rsf_census_call_state {
    uint16_t fn;
    bool full;
    uint32_t rva, tid;
    uint64_t base, site_ord, ord, t_us;
    void* stack[RSF_CENSUS_RECORD_STACK_FRAMES];
    uint32_t stack_n;
    rsf_census_json json;
    char body[RSF_CENSUS_RECORD_BODY_MAX];
} rsf_census_call_state;

#endif
