// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// Byte layouts of the per-draw b13 constant buffers, exactly as games/anomaly/shaders/core/README.md and
// games/anomaly/shaders/foliage/README.md specify them (FXC-reflected offsets). Pure CPU; no D3D.
#include "../temporal/math.h"
#include <array>
#include <cstddef>
#include <cstdint>
#include <span>

namespace rsf::anomaly::motion {
// VS b13, core family (RSFCorePrevious): rsf_previous_wvp @0 (64), rsf_previous_bones[234] @64 (3744),
// rsf_history @3808 (16).
inline constexpr std::size_t core_vs_bytes = 3824;
inline constexpr std::size_t core_wvp_offset = 0, core_bones_offset = 64, core_history_offset = 3808;
inline constexpr std::size_t core_bone_rows = 234, core_max_bones = core_bone_rows / 3;  // 78 affine bones
// VS b13, foliage family (RSFFoliagePrevious). RSF currently supplies no foliage history, so only the
// all-zero "unsupported draw" buffer is produced; its history uint4 at @160 is zero and therefore invalid.
inline constexpr std::size_t foliage_vs_bytes = 5056;
inline constexpr std::size_t foliage_history_offset = 160;
// PS b13 (RSFCorePixel): rsf_viewport @0, rsf_jitter @16, rsf_pixel_control @32.
inline constexpr std::size_t ps_bytes = 48;
inline constexpr std::size_t ps_viewport_offset = 0, ps_jitter_offset = 16, ps_control_offset = 32;
inline constexpr float abi_version = 1.0f;

using CoreVertexBytes = std::array<std::byte, core_vs_bytes>;
using FoliageVertexBytes = std::array<std::byte, foliage_vs_bytes>;
using PixelBytes = std::array<std::byte, ps_bytes>;

// The meaning of each rsf_history lane. Only the exact value 1.0f is accepted by the shader.
struct CoreHistory {
    bool draw{};      // X: the matching prior draw exists (object, topology, deformation mode, view, frame identity)
    bool palette{};   // Y: the prior palette exists (SKIN_1..4)
    bool rigid{};     // Z: SKIN_NONE draw is certified rigid and history-stable
};
// previous_wvp is the column-vector matrix M with previous clip = M * object position (prior P * V * W, possibly
// bone-composed). Its rows are stored as rows (row_major float4x4, mul(M, v)). bones are column-vector affine bone
// matrices in native palette order; rows 0..2 of bone i go to rsf_previous_bones[3i..3i+2], which is exactly the
// engine's own CSkeletonX upload (M._11 M._21 M._31 M._41 per row of the row-vector Fmatrix).
// Returns false and writes an all-invalid buffer (history X/Y/Z zero, ABI 1) when the inputs are not finite or
// there are more than 78 bones; palette is then also reported false.
bool pack_core_vertex(const temporal::Mat4& previous_wvp, std::span<const temporal::Mat4> bones, CoreHistory history,
    CoreVertexBytes& out) noexcept;
// An initialized invalid core buffer: zero matrix, zero palette, rsf_history = (0, 0, 0, 1).
void pack_core_invalid(CoreVertexBytes& out) noexcept;
// The initialized zero foliage buffer the foliage README requires for unsupported draws.
void pack_foliage_invalid(FoliageVertexBytes& out) noexcept;

struct PixelInputs {
    float viewport_x{}, viewport_y{}, viewport_width{}, viewport_height{};  // current viewport in RT pixels
    float current_jitter_u{}, current_jitter_v{};    // current jitter, normalized viewport UV, Y down
    float previous_jitter_u{}, previous_jitter_v{};  // previous jitter; MUST be zero for foliage draws
};
// Returns false (and writes control.x = 0, which makes the shader write invalid motion) for a non-positive or
// non-finite viewport extent.
bool pack_pixel(const PixelInputs&, PixelBytes& out) noexcept;

// Row-vector engine Fmatrix bytes (16 floats, _11 _12 ... _44) to the column-vector Mat4 used here.
temporal::Mat4 from_engine(const float (&values)[16]) noexcept;
// M = a * b for column-vector matrices, without the throwing accessors.
temporal::Mat4 compose(const temporal::Mat4& a, const temporal::Mat4& b) noexcept;
}
