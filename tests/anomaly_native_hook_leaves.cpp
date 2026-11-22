// SPDX-License-Identifier: MIT
// Native target functions and the existing exact profiles for the Rust fixture.
#include "build_profile.h"
#include "native_memory.h"
#include "native_hooks.h"

namespace {
using namespace rsf::anomaly;
using Function = int(*)(int);
__declspec(noinline) int target(int value) { return value + 7; }
__declspec(noinline) int untouched(int value) { return value * 5 + 11; }

constexpr std::array sr_detours{sr::site_phase_scene_prepare, sr::site_phase_combine,
    sr::site_phase_sunshafts, sr::site_phase_blur, sr::site_sun_cascades};
static_assert(sr_detours.size() == 5);

struct SiteBank {
    const NativeSite* sites;
    size_t count;
};
// The SR site bank of each build; the motion sites are Rust-owned (motion_profile.rs).
struct FixtureProfile {
    SiteBank sr;
};
static_assert(sizeof(SiteBank) == 16);
static_assert(sizeof(FixtureProfile) == 16);
constexpr auto fixture_profiles = [] {
    std::array<FixtureProfile, build_profiles.size()> result{};
    for (size_t build = 0; build < build_profiles.size(); ++build) {
        const auto& profile = build_profiles[build];
        const auto& sr = profile.sr->sr_function_sites;
        result[build] = {{sr.data(), sr.size()}};
    }
    return result;
}();
}

extern "C" int rsf_test_anomaly_native_hooks(const void*, decltype(&rsf::anomaly::guarded_copy),
    Function, Function, const FixtureProfile*, size_t, const size_t*) noexcept;

int main() {
    return rsf_test_anomaly_native_hooks(&rsf_anomaly_native_hook_platform, rsf::anomaly::guarded_copy,
        target, untouched, fixture_profiles.data(), fixture_profiles.size(), sr_detours.data());
}
