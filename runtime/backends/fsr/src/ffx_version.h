// SPDX-License-Identifier: MIT
#pragma once
#include <cstddef>
#include <cstdint>
#include <cstdio>

struct rsf_ffx_version {
    uint64_t id;
    const char* name;
    uint32_t major, minor, patch;
};
static_assert(sizeof(rsf_ffx_version) == 32 && offsetof(rsf_ffx_version, major) == 16);

namespace rsf {
inline constexpr size_t max_ffx_versions = 32;

inline rsf_ffx_version ffx_version(uint64_t id, const char* name, bool* parsed = nullptr)
{
    rsf_ffx_version version{id, name, 0, 0, 0};
    const bool numeric = name && std::sscanf(name, "%u.%u.%u", &version.major, &version.minor, &version.patch) == 3;
    if (!numeric) version.major = 0;
    if (parsed) *parsed = numeric;
    return version;
}

}
