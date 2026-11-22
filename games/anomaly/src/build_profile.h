// SPDX-License-Identifier: MIT
#pragma once
#include "geometry_profile.h"
#include "native_profile.h"
#include "sr_profile.h"
#include <cstddef>
#include <string_view>

namespace rsf::anomaly {
// Native consumers use a typed view at the index admitted by Rust. The motion profile is Rust-owned
// (motion_profile.rs, same build order).
struct BuildProfile {
    const char* label;               // log text: "standard", "avx"
    const char* executable;          // file name as shipped (ASCII)
    const char* sha256;              // lowercase hex
    const NativeProfile* native;
    const geometry::GeometryProfile* geometry;
    const sr::SrProfile* sr;
    constexpr uint32_t image_size() const noexcept { return native->image_size; }
};
inline constexpr std::array<BuildProfile, 2> build_profiles{{
    {standard_label, standard_executable, dx11_hash, &native_standard, &geometry::geometry_standard,
        &sr::sr_standard},
    {avx_label, avx_executable, avx_hash, &native_avx, &geometry::geometry_avx,
        &sr::sr_avx},
}};
inline constexpr const BuildProfile& standard_build = build_profiles[0];
inline constexpr const BuildProfile& avx_build = build_profiles[1];
}
