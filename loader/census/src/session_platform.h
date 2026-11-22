/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_SESSION_PLATFORM_H
#define RSF_CENSUS_SESSION_PLATFORM_H

#include "record_platform.h"
#include "vendor_platform.h"

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

typedef struct rsf_census_core_platform {
    const rsf_census_hook_desc* functions;
    uint16_t count;
    uint16_t event_entry;
    const rsf_census_hook_platform* hooks;
    const rsf_census_vendor_platform* vendors;
} rsf_census_core_platform;

#ifdef __cplusplus
extern "C" {
#endif
rsf_census_result rsf_census_install_impl(const rsf_census_options*, const rsf_census_core_platform*);
#ifdef __cplusplus
}
#endif
#endif
