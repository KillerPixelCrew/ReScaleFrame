// SPDX-License-Identifier: GPL-3.0-only
// Test helper: which exact Anomaly build profile (games/anomaly/src/build_profile.h) a fixture runs against. CTest
// registers the profile-dependent fixtures once per build; the build is the argument "standard" (default) or "avx".
#pragma once
#include "build_profile.h"
#include <cstdio>
#include <cstdlib>
#include <string_view>

namespace rsf::anomaly::testing {
inline const BuildProfile& selected_build(int argc, char** argv) noexcept
{
    for (int i = 1; i < argc; ++i) {
        const std::string_view value = argv[i];
        for (const auto& build : build_profiles)
            if (value == build.label) return build;
    }
    return standard_build;
}
// The profile of the other executable: its tables must never be accepted on this build's image.
inline const BuildProfile& other_build(const BuildProfile& build) noexcept
{
    return &build == &standard_build ? avx_build : standard_build;
}
inline void announce(const BuildProfile& build) noexcept
{
    std::printf("build profile: %s (%s)\n", build.label, build.executable);
}
}
