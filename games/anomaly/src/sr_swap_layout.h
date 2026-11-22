// SPDX-License-Identifier: MIT
#pragma once
#include "sr_swap.h"
#include "sr_profile.h"
#include <string_view>

namespace rsf::anomaly::sr {
// Every world target must be the profile's verified CRenderTarget slot with the same offset and format.
consteval bool world_targets_match_profile() {
    for (const auto& world : sr_world_targets) {
        bool found = false;
        for (const auto& item : target_slots)
            if (std::string_view{item.name} == std::string_view{world.name})
                found = item.offset == world.offset && item.d3d_format == world.d3d_format;
        if (!found) return false;
    }
    return std::string_view{sr_world_targets[RSF_ANOMALY_SWAP_SLOT_GENERIC0].name} == "$user$generic0" &&
        std::string_view{sr_world_targets[RSF_ANOMALY_SWAP_SLOT_GENERIC_TEMP].name} == "$user$generic_temp" &&
        std::string_view{sr_world_targets[RSF_ANOMALY_SWAP_SLOT_POSITION].name} == "$user$position" &&
        std::string_view{sr_world_targets[RSF_ANOMALY_SWAP_SLOT_GENERIC1].name} == "$user$generic1" &&
        sr_world_targets[RSF_ANOMALY_SWAP_SLOT_GENERIC0].d3d_format == d3dfmt_a8r8g8b8 &&
        sr_world_targets[RSF_ANOMALY_SWAP_SLOT_GENERIC_TEMP].d3d_format == d3dfmt_a8r8g8b8;
}
static_assert(sr_world_targets.size() == RSF_ANOMALY_SWAP_SLOT_COUNT);
static_assert(sr_world_absent_offsets.size() == RSF_ANOMALY_SWAP_ABSENT_COUNT);
static_assert(world_targets_match_profile());
static_assert(texture_desc_offset + RSF_ANOMALY_SWAP_DESC_SIZE <= texture_srv_offset);
static_assert(crt_format_offset + sizeof(uint32_t) <= crt_order_offset);
static_assert(ssao_viewport_height_offset == ssao_viewport_width_offset + sizeof(float));
static_assert(sizeof(rsf_anomaly_swap_fields) == 120 && offsetof(rsf_anomaly_swap_fields, desc) == 64);

// The layout shared by both executable profiles, in the Rust owner's form.
constexpr rsf_anomaly_swap_layout engine_layout() noexcept {
    rsf_anomaly_swap_layout layout{};
    layout.struct_size = sizeof(layout);
    layout.crt_surface = crt_surface_offset;
    layout.crt_rtv = crt_rtv_offset;
    layout.crt_dsv = crt_dsv_offset;
    layout.crt_uav = crt_uav_offset;
    layout.crt_texture = crt_texture_offset;
    layout.crt_width = crt_width_offset;
    layout.crt_height = crt_height_offset;
    layout.crt_format = crt_format_offset;
    layout.texture_surface = texture_surface_offset;
    layout.texture_desc_cache = texture_desc_cache_offset;
    layout.texture_desc = texture_desc_offset;
    layout.texture_srv = texture_srv_offset;
    layout.target_width = target_width_offset;
    layout.target_height = target_height_offset;
    layout.backend_texture_list = backend_texture_list_offset;
    layout.backend_ctable = backend_ctable_offset;
    layout.backend_textures[0] = {backend_textures_ps_offset, backend_textures_ps_count};
    layout.backend_textures[1] = {backend_textures_vs_offset, backend_textures_vs_count};
    layout.backend_textures[2] = {backend_textures_gs_offset, backend_textures_stage_count};
    layout.backend_textures[3] = {backend_textures_hs_offset, backend_textures_stage_count};
    layout.backend_textures[4] = {backend_textures_ds_offset, backend_textures_stage_count};
    layout.backend_textures[5] = {backend_textures_cs_offset, backend_textures_stage_count};
    layout.ssao_viewport_width = ssao_viewport_width_offset;
    layout.format_a16b16g16r16f = d3dfmt_a16b16g16r16f;
    layout.format_r16f = d3dfmt_r16f;
    layout.format_r32f = d3dfmt_r32f;
    for (std::size_t i = 0; i < RSF_ANOMALY_SWAP_SLOT_COUNT; ++i)
        layout.world[i] = {sr_world_targets[i].offset, sr_world_targets[i].d3d_format, sr_world_targets[i].optional,
            sr_world_targets[i].half_allowed};
    for (std::size_t i = 0; i < sr_world_absent_offsets.size(); ++i) layout.absent[i] = sr_world_absent_offsets[i];
    return layout;
}
inline constexpr rsf_anomaly_swap_layout verified_layout = engine_layout();

}
