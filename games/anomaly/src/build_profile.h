// SPDX-License-Identifier: MIT
#pragma once
#include "build_identity.h"
#include "geometry_profile.h"
#include "motion_profile.h"
#include "native_profile.h"
#include "sr_profile.h"
#include "input_profile.h"
#include <cstddef>
#include <string_view>

extern "C" size_t rsf_anomaly_match_build(const rsf_anomaly_build_identity*, size_t, const char*, size_t,
    const char*, size_t) noexcept;

extern "C" size_t rsf_anomaly_match_build_wide(const rsf_anomaly_build_identity*, size_t, const wchar_t*, size_t,
    const char*, size_t) noexcept;

namespace rsf::anomaly {
// One exact Anomaly executable: its file name, its SHA-256 and every native table the adapter uses on it. The adapter
// selects a build once, at startup, from the running executable's name and hash (find_build). There is no pattern scan
// and no fallback: an executable that matches no entry is refused, and one build's tables are never used on the other.
struct BuildProfile {
    const char* label;               // log text: "standard", "avx"
    const char* executable;          // file name as shipped (ASCII)
    const wchar_t* executable_wide;
    const char* sha256;              // lowercase hex
    const NativeProfile* native;
    const geometry::GeometryProfile* geometry;
    const sr::SrProfile* sr;
    const motion::MotionProfile* motion;
    const InputProfile* input;
    constexpr uint32_t image_size() const noexcept { return native->image_size; }
};
inline constexpr std::array<BuildProfile, 2> build_profiles{{
    {"standard", "AnomalyDX11.exe", L"AnomalyDX11.exe", dx11_hash, &native_standard, &geometry::geometry_standard,
        &sr::sr_standard, &motion::motion_standard, &input_standard},
    {"avx", "AnomalyDX11AVX.exe", L"AnomalyDX11AVX.exe", avx_hash, &native_avx, &geometry::geometry_avx,
        &sr::sr_avx, &motion::motion_avx, &input_avx},
}};
inline constexpr const BuildProfile& standard_build = build_profiles[0];
inline constexpr const BuildProfile& avx_build = build_profiles[1];
inline constexpr auto build_identities = [] {
    std::array<rsf_anomaly_build_identity, build_profiles.size()> identities{};
    for (size_t i = 0; i < build_profiles.size(); ++i)
        identities[i] = {build_profiles[i].executable, build_profiles[i].sha256};
    return identities;
}();

// The build whose file name (case-insensitive, as Windows resolves it) and SHA-256 (hex, either case) both match, or null.
inline const BuildProfile* find_build(std::string_view executable, std::string_view sha256) noexcept {
    const auto index = rsf_anomaly_match_build(build_identities.data(), build_identities.size(),
        executable.data(), executable.size(), sha256.data(), sha256.size());
    return index < build_profiles.size() ? &build_profiles[index] : nullptr;
}
// Same for a wide file name (GetModuleFileNameW). Anything outside ASCII matches no build.
inline const BuildProfile* find_build(std::wstring_view executable, std::string_view sha256) noexcept {
    const auto index = rsf_anomaly_match_build_wide(build_identities.data(), build_identities.size(),
        executable.data(), executable.size(), sha256.data(), sha256.size());
    return index < build_profiles.size() ? &build_profiles[index] : nullptr;
}

}
