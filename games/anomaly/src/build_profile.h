// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "geometry_profile.h"
#include "motion_profile.h"
#include "native_profile.h"
#include "sr_profile.h"
#include "input_profile.h"
#include <string_view>

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

namespace build_detail {
constexpr char lower(char c) noexcept { return c >= 'A' && c <= 'Z' ? static_cast<char>(c - 'A' + 'a') : c; }
constexpr bool iequal(std::string_view a, std::string_view b) noexcept {
    if (a.size() != b.size()) return false;
    for (size_t i = 0; i < a.size(); ++i) if (lower(a[i]) != lower(b[i])) return false;
    return true;
}
}
// The build whose file name (case-insensitive, as Windows resolves it) and SHA-256 (hex, either case) both match, or null.
constexpr const BuildProfile* find_build(std::string_view executable, std::string_view sha256) noexcept {
    for (const auto& build : build_profiles)
        if (build_detail::iequal(executable, build.executable) && build_detail::iequal(sha256, build.sha256)) return &build;
    return nullptr;
}
// Same for a wide file name (GetModuleFileNameW). Anything outside ASCII matches no build.
inline const BuildProfile* find_build(std::wstring_view executable, std::string_view sha256) noexcept {
    char narrow[64]{};
    if (executable.size() >= sizeof(narrow)) return nullptr;
    for (size_t i = 0; i < executable.size(); ++i) {
        if (executable[i] <= 0 || executable[i] > 0x7f) return nullptr;
        narrow[i] = static_cast<char>(executable[i]);
    }
    return find_build(std::string_view{narrow, executable.size()}, sha256);
}

static_assert(find_build("AnomalyDX11.exe", dx11_hash) == &standard_build);
static_assert(find_build("anomalydx11avx.EXE", avx_hash) == &avx_build);
static_assert(!find_build("AnomalyDX11.exe", avx_hash) && !find_build("AnomalyDX11AVX.exe", dx11_hash));
static_assert(!find_build("AnomalyDX10AVX.exe", avx_hash));
}
