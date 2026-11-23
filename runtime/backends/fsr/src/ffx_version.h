/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct rsf_ffx_version {
    uint64_t id;
    const char* name;
    uint32_t major, minor, patch;
} rsf_ffx_version;

#define RSF_FFX_MAX_VERSIONS 32u

#ifdef __cplusplus
static_assert(sizeof(rsf_ffx_version) == 32 && offsetof(rsf_ffx_version, major) == 16);
#endif
