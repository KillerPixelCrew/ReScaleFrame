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
extern "C" {
#endif

bool rsf_fsr4_parse_version(uint64_t id, const char* name, rsf_ffx_version* out);

#ifdef __cplusplus
}

namespace rsf {
inline constexpr size_t max_ffx_versions = RSF_FFX_MAX_VERSIONS;

inline rsf_ffx_version ffx_version(uint64_t id, const char* name, bool* parsed = nullptr)
{
    rsf_ffx_version version{};
    const bool numeric = rsf_fsr4_parse_version(id, name, &version);
    if (parsed) *parsed = numeric;
    return version;
}
}
#endif
