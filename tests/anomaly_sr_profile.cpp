// SPDX-License-Identifier: MIT
// Native projection of the independent profile contract's inputs.
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
    const void* owners[4];
    SiteBank sites[7];
    GuardBank guards[4];
    FirstPersonBank first_person;
    uint32_t required_rvas[2];
    uint32_t shader_target, sun_target, sr_near_index, geometry_near_index;
    const char* ssao_code;
    uint32_t ssao_function, ssao_viewport;
    Relation relations[13];
};
static_assert(sizeof(Profile) == 456 && offsetof(Profile, sites) == 64 &&
    offsetof(Profile, ssao_code) == 280 && offsetof(Profile, relations) == 296);
static_assert(sizeof(sr::FirstPersonSite) == 24 && sizeof(sr::TargetSlot) == 16);

constexpr auto profiles = [] {
    std::array<Profile, build_profiles.size()> result{};
    for (size_t i = 0; i < result.size(); ++i) {
        const auto& b = build_profiles[i];
        const auto& n = *b.native;
        const auto& g = *b.geometry;
        const auto& s = *b.sr;
        const auto& m = *b.motion;
        result[i] = {
            b.label, b.executable, b.sha256, b.image_size(), {b.native, b.geometry, b.sr, b.motion},
            {{n.native_sites.data(), n.native_sites.size()}, {g.geometry_sites.data(), g.geometry_sites.size()},
             {g.geometry_layout_sites.data(), g.geometry_layout_sites.size()},
             {s.sr_function_sites.data(), s.sr_function_sites.size()}, {s.sr_call_sites.data(), s.sr_call_sites.size()},
             {m.motion_sites.data(), m.motion_sites.size()}, {m.create_shader_calls.data(), m.create_shader_calls.size()}},
            {{n.native_unique_guards.data(), n.native_unique_guards.size()},
             {g.geometry_unique_guards.data(), g.geometry_unique_guards.size()},
             {s.sr_unique_guards.data(), s.sr_unique_guards.size()},
             {m.motion_unique_guards.data(), m.motion_unique_guards.size()}},
            {s.sr_first_person_sites.data(), s.sr_first_person_sites.size()},
            {s.get_constant_buffer_rva, s.position_setup_vtable_rva},
            m.motion_sites[motion::site_create_shader].rva, s.sr_function_sites[sr::site_sun_cascades].rva,
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

// C++ layout consumers still get their compile-time offset cross-checks.
// CRT field order follows SH_RT.h; pTexture is a ref_texture (one pointer) and the record ends after _order.
static_assert(sr::crt_surface_offset < sr::crt_rtv_offset && sr::crt_rtv_offset < sr::crt_dsv_offset &&
              sr::crt_dsv_offset < sr::crt_uav_offset && sr::crt_uav_offset < sr::crt_texture_offset &&
              sr::crt_texture_offset + 8 == sr::crt_width_offset && sr::crt_width_offset + 4 == sr::crt_height_offset &&
              sr::crt_height_offset + 4 == sr::crt_format_offset && sr::crt_order_offset + 8 == sr::crt_size);
// RCache texture caches: ps[16], vs[4], then gs/hs/ds/cs[16] contiguous; T precedes them (STextureList*, M, C).
static_assert(sr::backend_textures_vs_offset == sr::backend_textures_ps_offset + 8 * sr::backend_textures_ps_count);
static_assert(sr::backend_textures_gs_offset == sr::backend_textures_vs_offset + 8 * sr::backend_textures_vs_count);
static_assert(sr::backend_textures_hs_offset == sr::backend_textures_gs_offset + 8 * sr::backend_textures_stage_count);
static_assert(sr::backend_textures_cs_offset == sr::backend_textures_ds_offset + 8 * sr::backend_textures_stage_count);
static_assert(sr::backend_texture_list_offset + 24 == sr::backend_textures_ps_offset);
static_assert(sr::xforms_project_offset == sr::xforms_view_offset + 64);
}

extern "C" int rsf_test_anomaly_profile_contract(const Profile*, const rsf::anomaly::sr::TargetSlot*, size_t) noexcept;

int main() {
    return rsf_test_anomaly_profile_contract(profiles.data(), rsf::anomaly::sr::target_slots.data(),
        rsf::anomaly::sr::target_slots.size());
}
