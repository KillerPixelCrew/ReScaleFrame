// SPDX-License-Identifier: GPL-3.0-only
// Compile-time consistency of the per-build Anomaly profiles (games/anomaly/src/*profile*.h, build_profile.h), checked for
// the standard and the AVX table alike. This does not read the game executable: the byte guards and layouts are checked
// against the installed executables by tests/anomaly_{geometry,sr,motion,fg}_evidence.py --verify [--exe avx].
#include "build_profile.h"

#include <cstdio>
#include <span>
#include <string_view>

namespace {
using namespace rsf::anomaly;

constexpr bool unique_rvas(const sr::SrProfile& p) {
    for (std::size_t i = 0; i < p.sr_function_sites.size(); ++i)
        for (std::size_t j = i + 1; j < p.sr_function_sites.size(); ++j)
            if (p.sr_function_sites[i].rva == p.sr_function_sites[j].rva) return false;
    return true;
}
constexpr bool in_image(const BuildProfile& b, uint32_t rva) { return rva != 0 && rva < b.image_size(); }
constexpr bool sites_in_image(const BuildProfile& b) {
    for (const auto& site : b.native->native_sites) if (!in_image(b, site.rva)) return false;
    for (const auto& site : b.geometry->geometry_sites) if (!in_image(b, site.rva)) return false;
    for (const auto& site : b.geometry->geometry_layout_sites) if (!in_image(b, site.rva)) return false;
    for (const auto& site : b.sr->sr_function_sites) if (!in_image(b, site.rva)) return false;
    for (const auto& site : b.sr->sr_call_sites)
        if (!in_image(b, site.rva) || site.expected[0] != std::byte{0xe8}) return false;   // real call rel32
    for (const auto& site : b.motion->motion_sites) if (!in_image(b, site.rva)) return false;
    for (const auto& site : b.motion->create_shader_calls)
        if (!in_image(b, site.rva) || site.expected[0] != std::byte{0xe8}) return false;
    return true;
}
constexpr int nibble(char c) { return c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : -1; }
// Every extended guard is longer than 16 bytes, lower-case hex, and starts with the 16-byte guard of a same-named site at
// the same RVA in its domain table.
template<std::size_t N>
constexpr bool extended_guards_match(std::span<const UniqueGuard> guards, const std::array<NativeSite, N>& sites,
    const std::array<NativeSite, N>* extra = nullptr) {
    for (const auto& guard : guards) {
        const std::string_view hex{guard.expected_hex};
        if (hex.size() <= 32 || hex.size() % 2 != 0) return false;
        for (char c : hex)
            if (nibble(c) < 0) return false;
        bool found = false;
        const auto check = [&](const NativeSite& site) {
            if (site.rva != guard.rva || std::string_view{site.name} != guard.name) return true;
            for (std::size_t i = 0; i < 16; ++i)
                if (static_cast<int>(site.expected[i]) != nibble(hex[2 * i]) * 16 + nibble(hex[2 * i + 1])) return false;
            found = true;
            return true;
        };
        for (const auto& site : sites) if (!check(site)) return false;
        if (extra) for (const auto& site : *extra) if (!check(site)) return false;
        if (!found) return false;
    }
    return true;
}
constexpr bool geometry_guards_match(const geometry::GeometryProfile& p) {
    // Geometry guards cover the detours and the set_W layout guard (a different array length): check both tables.
    for (const auto& guard : p.geometry_unique_guards) {
        const std::span<const UniqueGuard> one{&guard, 1};
        if (!extended_guards_match(one, p.geometry_sites) && !extended_guards_match(one, p.geometry_layout_sites)) return false;
    }
    return true;
}
constexpr bool first_person_ordered(const sr::SrProfile& p) {
    for (const auto& site : p.sr_first_person_sites)
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
// The SSAO helper code ends with `lea r8,[rip+viewport]` (4C 8D 05 disp32) whose target is ssao_viewport_rva.
constexpr bool ssao_code_ends_with_viewport_lea(const sr::SrProfile& p) {
    const std::string_view hex{p.ssao_set_viewport_code};
    if (hex.size() < 14 || hex.size() % 2) return false;
    const auto bytes = hex.size() / 2;
    if (hex.substr(hex.size() - 14, 6) != "4c8d05") return false;
    uint32_t displacement = 0;
    for (int i = 3; i >= 0; --i) {
        const auto at = hex.size() - 8 + static_cast<std::size_t>(i) * 2;
        displacement = displacement << 8 | static_cast<uint32_t>(nibble(hex[at]) * 16 + nibble(hex[at + 1]));
    }
    return p.ssao_set_viewport_rva + bytes + static_cast<int32_t>(displacement) == p.ssao_viewport_rva;
}
// The create_shader call guards are rel32 calls to the create_shader detour.
constexpr bool create_calls_reach_detour(const motion::MotionProfile& p) {
    for (const auto& call : p.create_shader_calls) {
        int32_t displacement = 0;
        for (int i = 4; i >= 1; --i) displacement = static_cast<int32_t>(static_cast<uint32_t>(displacement) << 8 |
            static_cast<uint32_t>(call.expected[static_cast<std::size_t>(i)]));
        if (call.rva + 5 + displacement != p.motion_sites[motion::site_create_shader].rva) return false;
    }
    return true;
}
constexpr bool sun_call_reaches_wrapper(const sr::SrProfile& p) {
    const auto& call = p.sr_call_sites[5];
    uint32_t raw{};
    for (unsigned i = 0; i < 4; ++i) raw |= uint32_t(call.expected[i + 1]) << (8 * i);
    const auto displacement = raw <= 0x7fffffff ? int64_t(raw) : int64_t(raw) - 0x100000000;
    return int64_t(call.rva) + 5 + displacement == p.sr_function_sites[sr::site_sun_cascades].rva;
}
constexpr bool consistent(const BuildProfile& b) {
    const auto& n = *b.native; const auto& g = *b.geometry; const auto& s = *b.sr; const auto& m = *b.motion;
    return in_image(b, s.get_constant_buffer_rva) && in_image(b, s.position_setup_vtable_rva) &&
        sun_call_reaches_wrapper(s) && unique_rvas(s) && sites_in_image(b) && first_person_ordered(s) && ssao_code_ends_with_viewport_lea(s) &&
        create_calls_reach_detour(m) &&
        extended_guards_match(n.native_unique_guards, n.native_sites) && geometry_guards_match(g) &&
        extended_guards_match(s.sr_unique_guards, s.sr_function_sites) &&
        extended_guards_match(m.motion_unique_guards, m.motion_sites) &&
        // Shared facts between the domain tables of one build.
        s.backend_rva + sr::backend_xforms_offset == g.world_matrix_rva &&
        s.render_implementation_rva + 0x150 == g.render_phase_rva &&
        s.sr_function_sites[sr::site_rm_near].rva == g.geometry_sites[geometry::geometry_site_rm_near].rva &&
        s.sr_function_sites[sr::site_rm_near].expected == g.geometry_sites[geometry::geometry_site_rm_near].expected &&
        s.render_options_word3_rva == s.render_options_word0_rva + 12 &&
        s.device_project_rva == s.device_view_rva + 64 && s.device_full_transform_rva == s.device_project_rva + 64 &&
        s.device_precache_frame_rva == n.device_width_rva + 8 && n.device_height_rva == n.device_width_rva + 4 &&
        s.sr_first_person_sites[0].function_rva == g.geometry_sites[geometry::geometry_site_render_hud].rva &&
        s.sr_first_person_sites[2].function_rva == g.geometry_sites[geometry::geometry_site_render_sorted].rva &&
        s.sr_first_person_sites[3].function_rva == g.geometry_sites[geometry::geometry_site_render_emissive].rva &&
        s.sr_first_person_sites[4].function_rva == s.sr_function_sites[13].rva &&
        s.sr_first_person_sites[5].function_rva == s.sr_function_sites[14].rva;
}
// Two builds never share a table, a hash or a file name, and no site of one build is accepted by name on the other.
constexpr bool builds_distinct() {
    const auto& a = standard_build; const auto& b = avx_build;
    return a.native != b.native && a.geometry != b.geometry && a.sr != b.sr && a.motion != b.motion &&
        std::string_view{a.sha256} != b.sha256 && std::string_view{a.executable} != b.executable &&
        a.image_size() != b.image_size() && std::string_view{a.sha256}.size() == 64 && std::string_view{b.sha256}.size() == 64;
}

static_assert(consistent(standard_build), "standard build profile");
static_assert(consistent(avx_build), "AVX build profile");
static_assert(builds_distinct());
static_assert(find_build("AnomalyDX11.exe", dx11_hash) == &standard_build && find_build("AnomalyDX11AVX.exe", avx_hash) == &avx_build);
static_assert(!find_build("AnomalyDX11AVX.exe", dx11_hash) && !find_build("AnomalyDX11.exe", avx_hash) && !find_build("", ""));
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
static_assert(sr::xforms_project_offset == sr::xforms_view_offset + 64);
}

int main() {
    int failures = 0;
    // Runtime selection, the path bootstrap and the observer take: wide file names, either hash case, no partial match.
    struct Case { const wchar_t* name; const char* hash; const BuildProfile* expected; };
    const Case cases[] = {
        {L"AnomalyDX11.exe", dx11_hash, &standard_build},
        {L"anomalydx11.exe", "C857B6F49D32C62245611B26106862A773185533856E8B858A7D7E849ED2FB4E", &standard_build},
        {L"AnomalyDX11AVX.exe", avx_hash, &avx_build},
        {L"ANOMALYDX11AVX.EXE", "2032ACFC041E215508692EF4079EF504C7BBABF1793D5668084099F734B92F12", &avx_build},
        {L"AnomalyDX11AVX.exe", dx11_hash, nullptr},       // right hash, wrong executable
        {L"AnomalyDX11.exe", avx_hash, nullptr},
        {L"AnomalyDX10AVX.exe", avx_hash, nullptr},        // other renderers are not profiled
        {L"AnomalyDX11.exe", "c857b6f49d32c62245611b26106862a773185533856e8b858a7d7e849ed2fb4", nullptr},   // truncated
        {L"AnomalyDX11Ãƒâ€ž.exe", dx11_hash, nullptr},     // non-ASCII never matches
        {L"", "", nullptr},
    };
    for (const auto& c : cases) {
        if (find_build(std::wstring_view{c.name}, c.hash) != c.expected) {
            std::printf("FAIL: find_build(%ls) selected %s\n", c.name, find_build(std::wstring_view{c.name}, c.hash)
                ? find_build(std::wstring_view{c.name}, c.hash)->label : "none");
            ++failures;
        }
    }
    for (const auto& build : build_profiles)
        std::printf("%s (%s): image 0x%x; %zu native, %zu geometry + %zu layout, %zu SR function + %zu call-site, %zu motion sites; "
                    "unique guards native %zu geometry %zu SR %zu motion %zu; %zu first-person sites; %zu target slots\n",
            build.label, build.executable, build.image_size(), build.native->native_sites.size(),
            build.geometry->geometry_sites.size(), build.geometry->geometry_layout_sites.size(),
            build.sr->sr_function_sites.size(), build.sr->sr_call_sites.size(), build.motion->motion_sites.size(),
            build.native->native_unique_guards.size(), build.geometry->geometry_unique_guards.size(),
            build.sr->sr_unique_guards.size(), build.motion->motion_unique_guards.size(),
            build.sr->sr_first_person_sites.size(), sr::target_slots.size());
    std::printf("%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
