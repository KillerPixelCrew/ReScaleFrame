// SPDX-License-Identifier: MIT
// Native harness for the Rust SR refusal fixture. The rmNear case owns a real
// MinHook detour and calls the fixture target through the engine's Windows ABI.
#include "anomaly_build_select.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include "sr_stage.h"

#include <windows.h>
#include <bit>
#include <cstdio>
#include <cstring>

namespace {
using namespace rsf::anomaly;
const BuildProfile* B{};
int near_calls = 0, callback_calls = 0;
void* callback_self{};
__declspec(noinline) void __fastcall near_fixture(void* self) { ++near_calls; if (!self) std::abort(); }
void callback(void* self) noexcept { ++callback_calls; callback_self = self; }

struct NearCalls {
    void (*invoke)(void*);
    void (*set_callback)(bool);
    bool (*disable)(void*);
    void (*counts)(int*, int*, void**);
};

}

extern "C" int32_t rsf_anomaly_sr_hook_near(const NearCalls*, void*) noexcept;

namespace {

int exercise_near_slot() {
    const auto sites = geometry::native_geometry_hook_sites(*B);
    auto* base = reinterpret_cast<std::byte*>(GetModuleHandleW(nullptr));
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    const auto* pe = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    HookSite redirect = sites[geometry::geometry_site_rm_near];
    redirect.rva = static_cast<uint32_t>(reinterpret_cast<std::byte*>(&near_fixture) - base);
    std::memcpy(redirect.expected.data(), reinterpret_cast<const void*>(&near_fixture), 16);
    HookSet hooks;
    if (!hooks.install(base, pe->OptionalHeader.SizeOfImage, std::span<const HookSite>(&redirect, 1))) {
        std::puts("FAIL installing the geometry rmNear fixture detour");
        return 1;
    }
    const NearCalls calls{
        near_fixture,
        [](bool on) { geometry::geometry_set_rm_near_callback(on ? &callback : nullptr); },
        [](void* owner) { return static_cast<HookSet*>(owner)->disable(); },
        [](int* near_count, int* callbacks, void** seen) {
            *near_count = near_calls;
            *callbacks = callback_calls;
            *seen = callback_self;
        },
    };
    const auto failures = rsf_anomaly_sr_hook_near(&calls, &hooks);
    geometry::geometry_set_rm_near_callback(nullptr);
    return failures;
}

struct FixtureProfile {
    const NativeSite* functions;
    const UniqueGuard* unique;
    std::size_t unique_count;
    const NativeSite* calls;
    const sr::FirstPersonSite* first_person;
    const UniqueGuard* reconstruction;
    const char* ssao_code;
    uint32_t ssao_rva;
    uint32_t image_size;
};
struct FixtureCalls {
    bool (*configure)(const wchar_t*, rsf_game_log_fn, void*);
    bool (*install)(std::byte*, uint32_t, bool);
    bool (*active)();
    void (*frame_end)();
    void (*report)(rsf_game_log_fn, void*);
    bool (*quiesce)();
    bool (*jitter)(rsf_anomaly_sr_jitter_uv*);
};
static_assert(sizeof(NativeSite) == 32 && sizeof(UniqueGuard) == 24 && sizeof(sr::FirstPersonSite) == 24);
static_assert(sizeof(FixtureProfile) == 64 && sizeof(FixtureCalls) == 56 && sizeof(NearCalls) == 32);
static_assert(sr::sr_function_site_count == 17 && sr::sr_call_site_count == 6 &&
    sr::sr_first_person_site_count == 6 && std::tuple_size_v<decltype(sr::SrProfile::reconstruction_guards)> == 4);
static_assert(sr::site_phase_combine == 1 && sr::site_phase_blur == 3 && sr::site_rm_near == 4 &&
    sr::site_rm_far == 5 && sr::site_set_p == 6 && sr::site_set_mip_lod_bias == 9 && sr::site_sun_cascades == 15);

const FixtureCalls fixture_calls{
    [](const wchar_t* ini, rsf_game_log_fn log, void* user) { return sr::configure(ini, true, log, user); },
    [](std::byte* image, uint32_t size, bool outside) {
        if (!outside) return sr::install(image, size, *B);
        auto build = *B;
        auto profile = *B->sr;
        build.sr = &profile;
        profile.sr_call_sites[0].rva = build.image_size();
        return sr::install(image, size, build);
    },
    sr::active,
    sr::frame_end,
    sr::report,
    sr::quiesce,
    [](rsf_anomaly_sr_jitter_uv* out) {
        *out = std::bit_cast<rsf_anomaly_sr_jitter_uv>(sr::current_jitter_uv());
        return sr::current_jitter().applied;
    },
};
}

extern "C" int32_t rsf_anomaly_sr_hook_refusals(const FixtureProfile*, const FixtureCalls*) noexcept;

int main(int argc, char** argv) {
    B = &testing::selected_build(argc, argv);
    testing::announce(*B);
    auto failures = exercise_near_slot();
    const auto& sr = *B->sr;
    const FixtureProfile profile{sr.sr_function_sites.data(), sr.sr_unique_guards.data(), sr.sr_unique_guards.size(),
        sr.sr_call_sites.data(), sr.sr_first_person_sites.data(), sr.reconstruction_guards.data(),
        sr.ssao_set_viewport_code, sr.ssao_set_viewport_rva, B->image_size()};
    failures += rsf_anomaly_sr_hook_refusals(&profile, &fixture_calls);
    std::printf("%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
