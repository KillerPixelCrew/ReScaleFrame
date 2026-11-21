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

inline rsf_ffx_version ffx_version(uint64_t id, const char* name)
{
    rsf_ffx_version version{id, name, 0, 0, 0};
    if (name && std::sscanf(name, "%u.%u.%u", &version.major, &version.minor, &version.patch) != 3)
        version.major = 0;
    return version;
}

// Picks the newest provider of one FidelityFX major version out of an ffxQuery GetVersions answer.
// IDs are opaque; the family comes from the SDK's paired "major.minor.patch" display name, never
// from bit masks. `wanted_id` of zero accepts any id. Returns false when nothing matches.
inline bool select_ffx_version(const uint64_t* ids, const char* const* names, uint64_t count, unsigned major,
                               uint64_t wanted_id, uint64_t& id, char* name, size_t name_size)
{
    bool found = false;
    unsigned best_minor = 0, best_patch = 0;
    for (uint64_t i = 0; i < count; ++i) {
        const auto version = ffx_version(ids[i], names[i]);
        if (!version.major || version.major != major || (wanted_id && version.id != wanted_id)) continue;
        if (!found || version.minor > best_minor || (version.minor == best_minor && version.patch > best_patch)) {
            found = true; id = version.id; best_minor = version.minor; best_patch = version.patch;
            std::snprintf(name, name_size, "%s", version.name);
        }
    }
    return found;
}
}
