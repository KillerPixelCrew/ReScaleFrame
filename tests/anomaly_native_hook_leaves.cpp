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
constexpr auto sr_sites = [] {
    std::array<std::array<NativeSite, sr_detours.size()>, build_profiles.size()> result{};
    for (size_t build = 0; build < build_profiles.size(); ++build) {
        const auto& bank = build_profiles[build].sr->sr_function_sites;
        for (size_t slot = 0; slot < sr_detours.size(); ++slot) result[build][slot] = bank[sr_detours[slot]];
    }
    return result;
}();

struct SiteBank {
    const NativeSite* sites;
    size_t count;
};
struct FixtureProfile {
    const char* label;
    uint32_t image_size;
    SiteBank banks[3];
};
static_assert(sizeof(SiteBank) == 16);
static_assert(sizeof(FixtureProfile) == 64 && offsetof(FixtureProfile, banks) == 16);
constexpr auto fixture_profiles = [] {
    std::array<FixtureProfile, build_profiles.size()> result{};
    for (size_t build = 0; build < build_profiles.size(); ++build) {
        const auto& profile = build_profiles[build];
        const auto& native = profile.native->native_sites;
        const auto& motion = profile.motion->motion_sites;
        const auto& sr = sr_sites[build];
        result[build] = {profile.label, profile.image_size(),
            {{native.data(), native.size()}, {motion.data(), motion.size()}, {sr.data(), sr.size()}}};
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
