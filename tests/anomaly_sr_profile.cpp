// SPDX-License-Identifier: MIT
// Native projection of the independent profile contract's inputs. The motion record is Rust-owned
// (motion_profile.rs); the contract reads it there.
#include "build_profile.h"

namespace {
using namespace rsf::anomaly;

struct SiteBank { const NativeSite* data; size_t count; };
struct GuardBank { const UniqueGuard* data; size_t count; };
struct FirstPersonBank { const sr::FirstPersonSite* data; size_t count; };
struct Relation { uint32_t base, offset, observed; };
struct Profile {
    const char* label;
    const char* executable;
    const char* sha256;
    uint32_t image_size;
    const void* owners[3];
    SiteBank sites[5];
    GuardBank guards[3];
    FirstPersonBank first_person;
    uint32_t required_rvas[2];
    uint32_t sun_target, sr_near_index, geometry_near_index;
    const char* ssao_code;
    uint32_t ssao_function, ssao_viewport;
    Relation relations[13];
};
static_assert(sizeof(Profile) == 400 && offsetof(Profile, sites) == 56 &&
    offsetof(Profile, ssao_code) == 224 && offsetof(Profile, relations) == 240);
static_assert(sizeof(sr::FirstPersonSite) == 24 && sizeof(sr::TargetSlot) == 16);

constexpr auto profiles = [] {
    std::array<Profile, build_profiles.size()> result{};
    for (size_t i = 0; i < result.size(); ++i) {
        const auto& b = build_profiles[i];
        const auto& n = *b.native;
        const auto& g = *b.geometry;
        const auto& s = *b.sr;
        result[i] = {
            b.label, b.executable, b.sha256, b.image_size(), {b.native, b.geometry, b.sr},
            {{n.native_sites.data(), n.native_sites.size()}, {g.geometry_sites.data(), g.geometry_sites.size()},
             {g.geometry_layout_sites.data(), g.geometry_layout_sites.size()},
             {s.sr_function_sites.data(), s.sr_function_sites.size()}, {s.sr_call_sites.data(), s.sr_call_sites.size()}},
            {{n.native_unique_guards.data(), n.native_unique_guards.size()},
             {g.geometry_unique_guards.data(), g.geometry_unique_guards.size()},
             {s.sr_unique_guards.data(), s.sr_unique_guards.size()}},
            {s.sr_first_person_sites.data(), s.sr_first_person_sites.size()},
            {s.get_constant_buffer_rva, s.position_setup_vtable_rva},
            s.sr_function_sites[sr::site_sun_cascades].rva,
            sr::site_rm_near, geometry::geometry_site_rm_near,
            s.ssao_set_viewport_code, s.ssao_set_viewport_rva, s.ssao_viewport_rva,
            {{s.backend_rva, sr::backend_xforms_offset, g.world_matrix_rva},
             {s.render_implementation_rva, 0x150, g.render_phase_rva},
             {s.render_options_word0_rva, 12, s.render_options_word3_rva},
             {s.device_view_rva, 64, s.device_project_rva},
             {s.device_project_rva, 64, s.device_full_transform_rva},
             {n.device_width_rva, 8, s.device_precache_frame_rva},
             {n.device_width_rva, 4, n.device_height_rva},
             {s.sr_first_person_sites[0].function_rva, 0, g.geometry_sites[geometry::geometry_site_render_hud].rva},
             {s.sr_first_person_sites[2].function_rva, 0, g.geometry_sites[geometry::geometry_site_render_sorted].rva},
             {s.sr_first_person_sites[3].function_rva, 0, g.geometry_sites[geometry::geometry_site_render_emissive].rva},
             {s.sr_first_person_sites[4].function_rva, 0, s.sr_function_sites[13].rva},
             {s.sr_first_person_sites[5].function_rva, 0, s.sr_function_sites[14].rva},
             {s.sr_function_sites[sr::site_rm_near].rva, 0, g.geometry_sites[geometry::geometry_site_rm_near].rva}},
        };
    }
    return result;
}();

// The SR layout relationships (CRT field order, RCache texture ranges, R_xforms) are compile-time checks in
// sr_profile.rs, which owns those values.
}

extern "C" int rsf_test_anomaly_profile_contract(const Profile*, const rsf::anomaly::sr::TargetSlot*, size_t) noexcept;

int main() {
    return rsf_test_anomaly_profile_contract(profiles.data(), rsf::anomaly::sr::target_slots.data(),
        rsf::anomaly::sr::target_slots.size());
}
