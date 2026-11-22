// SPDX-License-Identifier: MIT
// Native target functions and the existing exact profiles for the Rust fixture.
#include "build_profile.h"
#include "native_memory.h"

namespace {
using namespace rsf::anomaly;
using Function = int(*)(int);
__declspec(noinline) int target(int value) { return value + 7; }
__declspec(noinline) int untouched(int value) { return value * 5 + 11; }

constexpr std::array sr_detours{sr::site_phase_scene_prepare, sr::site_phase_combine,
    sr::site_phase_sunshafts, sr::site_phase_blur, sr::site_sun_cascades};
constexpr auto site_count = native_site_count + motion::motion_site_count + sr_detours.size();
constexpr auto native_sites = [] {
    std::array<std::array<NativeSite, site_count>, build_profiles.size()> result{};
    for (size_t build = 0; build < build_profiles.size(); ++build) {
        const auto& profile = build_profiles[build];
        size_t slot = 0;
        for (const auto& site : profile.native->native_sites) result[build][slot++] = site;
        for (const auto& site : profile.motion->motion_sites) result[build][slot++] = site;
        for (const auto site : sr_detours) result[build][slot++] = profile.sr->sr_function_sites[site];
    }
    return result;
}();

struct FixtureProfile {
    const char* label;
    uint32_t image_size;
    const NativeSite* sites;
    size_t count;
};
static_assert(sizeof(FixtureProfile) == 32 && offsetof(FixtureProfile, sites) == 16);
constexpr auto fixture_profiles = [] {
    std::array<FixtureProfile, build_profiles.size()> result{};
    for (size_t build = 0; build < build_profiles.size(); ++build) {
        result[build] = {build_profiles[build].label, build_profiles[build].image_size(),
            native_sites[build].data(), native_sites[build].size()};
    }
    return result;
}();
}

extern "C" const void* rsf_anomaly_hook_native_platform() noexcept;
extern "C" int rsf_test_anomaly_native_hooks(const void*, decltype(&rsf::anomaly::guarded_copy),
    Function, Function, const FixtureProfile*, size_t) noexcept;

int main() {
    return rsf_test_anomaly_native_hooks(rsf_anomaly_hook_native_platform(), rsf::anomaly::guarded_copy,
        target, untouched, fixture_profiles.data(), fixture_profiles.size());
}
