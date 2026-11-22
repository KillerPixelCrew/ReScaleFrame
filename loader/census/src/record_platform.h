/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_RECORD_PLATFORM_H
#define RSF_CENSUS_RECORD_PLATFORM_H

#include "census_platform.h"

#define RSF_CENSUS_RECORD_BODY_MAX 6144
#define RSF_CENSUS_RECORD_STACK_FRAMES 32
#define RSF_CENSUS_RECORD_THREAD_NAME 40

typedef struct rsf_census_queued_record {
    uint32_t len;
    uint16_t fn;
    uint64_t t_us, present, ord, site_ord, base, ret;
    uint32_t rva, tid, stack_n;
    void* stack[RSF_CENSUS_RECORD_STACK_FRAMES];
    char thread[RSF_CENSUS_RECORD_THREAD_NAME];
    char body[RSF_CENSUS_RECORD_BODY_MAX];
} rsf_census_queued_record;


#endif
