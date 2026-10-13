// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <cstddef>
#include <cstdint>
#include <vector>
#include <string>
enum class rsf_ac7_velocity_shader_result { refused, patched };

struct rsf_ac7_velocity_semantic {
    std::string name;
    uint32_t index=0, system=0, type=0, reg=0;
    uint8_t mask=0, used=0;
};
struct rsf_ac7_velocity_vertex_info {
    uint32_t particle=0;
    uint32_t position_register=0;
    std::vector<rsf_ac7_velocity_semantic> outputs;
};
bool rsf_ac7_velocity_signature(const void* bytes,size_t size,bool output,
    std::vector<rsf_ac7_velocity_semantic>& semantics) noexcept;
rsf_ac7_velocity_shader_result rsf_ac7_velocity_vertex_shader(const void* bytes,size_t size,
    std::vector<uint8_t>& output,rsf_ac7_velocity_vertex_info& info) noexcept;
bool rsf_ac7_velocity_geometry_shader(const rsf_ac7_velocity_vertex_info& info,
    std::vector<uint8_t>& bytecode,uint32_t& previous_register,uint32_t& current_register) noexcept;

// Retain the native material evaluation/discards, recover additive opacity, and replace its
// colour output with packed velocity. RSFPREV0 is the previous generated clip position.
// The replay binds Rect/Jitter/Cutoff in b13; the original game shader is never changed.
rsf_ac7_velocity_shader_result rsf_ac7_velocity_pixel_shader(const void* bytes, size_t size,
    uint32_t blend, std::vector<uint8_t>& output,uint32_t previous_register=UINT32_MAX,
    uint32_t current_register=UINT32_MAX) noexcept;
