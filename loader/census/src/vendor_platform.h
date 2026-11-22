/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_VENDOR_PLATFORM_H
#define RSF_CENSUS_VENDOR_PLATFORM_H

#include "fsr_platform.h"
#include "misc_platform.h"
#include "ngx_platform.h"
#include "wrapper_platform.h"

typedef struct rsf_census_vendor_platform {
    const uint16_t* vk_entries;
    const rsf_census_vk_platform* vk;
    const uint16_t* ngx_entries;
    const rsf_census_ngx_platform* ngx;
    const uint16_t* fsr_entries;
    const rsf_census_fsr_platform* fsr;
    const uint16_t* misc_entries;
    const rsf_census_misc_platform* misc;
} rsf_census_vendor_platform;

#endif
