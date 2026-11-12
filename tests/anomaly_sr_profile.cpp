// SPDX-License-Identifier: GPL-3.0-only
// Compile-time consistency of games/anomaly/src/sr_profile.h. This does not read the game executable: the byte
// guards and layouts are checked against the installed exe by tests/anomaly_sr_evidence.py --verify.
#include "sr_profile.h"
#include "geometry_profile.h"

#include <cstdio>
#include <string_view>

namespace {
using namespace rsf::anomaly;

constexpr bool unique_rvas() {
    for (std::size_t i = 0; i < sr::sr_function_sites.size(); ++i)
        for (std::size_t j = i + 1; j < sr::sr_function_sites.size(); ++j)
            if (sr::sr_function_sites[i].rva == sr::sr_function_sites[j].rva) return false;
    return true;
}
constexpr bool in_image(uint32_t rva) { return rva != 0 && rva < native_image_size; }
constexpr bool sites_in_image() {
    for (const auto& site : sr::sr_function_sites)
        if (!in_image(site.rva)) return false;
    for (const auto& site : sr::sr_call_sites)
        if (!in_image(site.rva) || site.expected[0] != std::byte{0xe8}) return false;   // real call rel32
    return true;
}
constexpr int nibble(char c) { return c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : -1; }
// Every extended guard is longer than 16 bytes, lower-case hex, and starts with the 16-byte guard of the same site.
constexpr bool extended_guards_match() {
    for (const auto& guard : sr::sr_unique_guards) {
        const std::string_view hex{guard.expected_hex};
        if (hex.size() <= 32 || hex.size() % 2 != 0) return false;
        for (char c : hex)
            if (nibble(c) < 0) return false;
        bool found = false;
        for (const auto& site : sr::sr_function_sites) {
            if (site.rva != guard.rva || std::string_view{site.name} != guard.name) continue;
            for (std::size_t i = 0; i < 16; ++i)
                if (static_cast<int>(site.expected[i]) != nibble(hex[2 * i]) * 16 + nibble(hex[2 * i + 1])) return false;
            found = true;
        }
        if (!found) return false;
    }
    return true;
}
constexpr bool first_person_ordered() {
    for (const auto& site : sr::sr_first_person_sites)
        if (!(site.function_rva < site.set_p_call && site.set_p_call < site.rm_near_call &&
              site.rm_near_call - site.set_p_call < 0x120))
            return false;
    return true;
}
constexpr bool target_slots_distinct() {
    for (std::size_t i = 0; i < sr::target_slots.size(); ++i) {
        if (sr::target_slots[i].offset % 8 != 0) return false;
        for (std::size_t j = i + 1; j < sr::target_slots.size(); ++j)
            if (sr::target_slots[i].offset == sr::target_slots[j].offset ||
                std::string_view{sr::target_slots[i].name} == sr::target_slots[j].name)
                return false;
    }
    return true;
}
constexpr const sr::TargetSlot& slot(std::string_view name) {
    for (const auto& s : sr::target_slots)
        if (s.name == name) return s;
    throw "missing target slot";
}

static_assert(unique_rvas());
static_assert(sites_in_image());
static_assert(extended_guards_match());
static_assert(first_person_ordered());
static_assert(target_slots_distinct());
// Seam layout used by Stage 1: generic0/generic_temp are the FP16-promoted world twins; rt_ui_pda and rt_secondVP are
// the two slots CRender::RenderToTarget already reads (+0x3D0/+0x3C0, anomaly-native-hooks.md).
static_assert(slot("$user$generic0").offset == 0x3a8 && slot("$user$generic0").d3d_format == 21);
static_assert(slot("$user$generic_temp").offset == 0x3b8 && slot("$user$generic1").offset == 0x3b0);
static_assert(slot("$user$ui").offset == 0x3d0 && slot("$user$viewport2").offset == 0x3c0);
static_assert(slot("$user$position").d3d_format == 113 && slot("$user$albedo").d3d_format == 21);
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
// Shared facts with the other Anomaly profiles.
static_assert(sr::backend_rva + sr::backend_xforms_offset == geometry::world_matrix_rva);
static_assert(sr::render_implementation_rva + 0x150 == geometry::render_phase_rva);
static_assert(sr::sr_function_sites[sr::site_rm_near].rva == geometry::geometry_sites[geometry::geometry_site_rm_near].rva &&
              sr::sr_function_sites[sr::site_rm_near].expected == geometry::geometry_sites[geometry::geometry_site_rm_near].expected);
static_assert(sr::xforms_project_offset == sr::xforms_view_offset + 64);
static_assert(sr::render_options_word3_rva == sr::render_options_word0_rva + 12);
}

int main() {
    std::printf("sr_profile: %zu function, %zu call-site, %zu extended guards; %zu first-person sites; %zu target slots\n",
                sr::sr_function_sites.size(), sr::sr_call_sites.size(), sr::sr_unique_guards.size(),
                sr::sr_first_person_sites.size(), sr::target_slots.size());
    return 0;
}
