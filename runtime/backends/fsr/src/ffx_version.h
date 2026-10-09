// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <cstdint>
#include <cstdio>

namespace rsf {
// Picks the newest provider of one FidelityFX major version out of an ffxQuery GetVersions answer.
// IDs are opaque; the family comes from the SDK's paired "major.minor.patch" display name, never
// from bit masks. `wanted_id` of zero accepts any id. Returns false when nothing matches.
inline bool select_ffx_version(const uint64_t* ids, const char* const* names, uint64_t count, unsigned major,
                               uint64_t wanted_id, uint64_t& id, char* name, size_t name_size)
{
    bool found = false;
    unsigned best_minor = 0, best_patch = 0;
    for (uint64_t i = 0; i < count; ++i) {
        unsigned found_major = 0, minor = 0, patch = 0;
        if (!names[i] || std::sscanf(names[i], "%u.%u.%u", &found_major, &minor, &patch) != 3 ||
            found_major != major || (wanted_id && ids[i] != wanted_id)) continue;
        if (!found || minor > best_minor || (minor == best_minor && patch > best_patch)) {
            found = true; id = ids[i]; best_minor = minor; best_patch = patch;
            std::snprintf(name, name_size, "%s", names[i]);
        }
    }
    return found;
}
}
