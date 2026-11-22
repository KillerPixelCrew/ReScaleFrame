// SPDX-License-Identifier: MIT
// Test helper: which exact Anomaly build profile (games/anomaly/src/build_profile.h) a fixture runs against. CTest
// registers the profile-dependent fixtures once per build; the build is the argument "standard" (default) or "avx".
#pragma once
#include "build_profile.h"
#include <cstdio>

extern "C" size_t rsf_test_anomaly_select_build(int, const char* const*, const char* const*, size_t) noexcept;

namespace rsf::anomaly::testing {
inline const BuildProfile& selected_build(int argc, char** argv) noexcept
{
    constexpr auto labels = [] {
        std::array<const char*, build_profiles.size()> result{};
        for (size_t i = 0; i < result.size(); ++i) result[i] = build_profiles[i].label;
        return result;
    }();
    return build_profiles[rsf_test_anomaly_select_build(argc, argv, labels.data(), labels.size())];
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
