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
// VS b13, foliage family (RSFFoliagePrevious, games/anomaly/shaders/foliage/rsf_anomaly_foliage.h and README):
// rsf_previous_clip_from_world @0 (64), rsf_previous_tree_xform float3x4 @64 (48), rsf_previous_consts @112,
// rsf_previous_wave @128, rsf_previous_wind (tree wind / detail dir2D) @144, rsf_foliage_history uint4 @160
// (complete, detail group count, ABI 1, 0), rsf_previous_rows[244] @176 (3904), rsf_previous_item_state[61] @4080 (976).
// An unsupported draw gets the all-zero buffer: its history uint4 is zero and therefore invalid.
inline constexpr std::size_t foliage_vs_bytes = 5056;
inline constexpr std::size_t foliage_clip_offset = 0, foliage_tree_xform_offset = 64, foliage_consts_offset = 112,
    foliage_wave_offset = 128, foliage_wind_offset = 144, foliage_history_offset = 160, foliage_rows_offset = 176,
    foliage_item_state_offset = 4080;
inline constexpr std::size_t foliage_detail_groups = 61, foliage_detail_rows = foliage_detail_groups * 4;
inline constexpr std::size_t foliage_detail_floats = foliage_detail_rows * 4;
// PS b13 (RSFCorePixel): rsf_viewport @0, rsf_jitter @16, rsf_pixel_control @32.
inline constexpr std::size_t ps_bytes = 48;
inline constexpr std::size_t ps_viewport_offset = 0, ps_jitter_offset = 16, ps_control_offset = 32;
inline constexpr float abi_version = 1.0f;
// DS b13, tessellation chain (RSFTessPrevious, games/anomaly/shaders/core/rsf_core_tess.h): rsf_previous_wv @0 (64),
// rsf_previous_projection @64 (64), rsf_tess_history @128 (16: valid, 0, 0, ABI).
inline constexpr std::size_t tess_ds_bytes = 144;
inline constexpr std::size_t tess_wv_offset = 0, tess_projection_offset = 64, tess_history_offset = 128;

using CoreVertexBytes = std::array<std::byte, core_vs_bytes>;
using TessDomainBytes = std::array<std::byte, tess_ds_bytes>;
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
// previous_wv takes the motion VS's previous object-space control data (the same space rsf_previous_wvp maps from) to
// the previous view space the PN patch is rebuilt in; previous_projection takes that to previous clip. Both
// column-vector, stored as rows. Returns false and writes the invalid buffer (history (0, 0, 0, 1)) when a value is not
// finite; `valid` false also writes it.
bool pack_tess_domain(const temporal::Mat4& previous_wv, const temporal::Mat4& previous_projection, bool valid,
    TessDomainBytes& out) noexcept;
void pack_tess_invalid(TessDomainBytes& out) noexcept;

// Previous-frame wind state of one foliage draw: tree consts/wave/wind, or detail consts/wave/dir2D.
struct FoliageConstants {
    float consts[4]{}, wave[4]{}, wind[4]{};
    bool operator==(const FoliageConstants&) const = default;
};
// One detail instance remapped to a CURRENT group of four rows. rows are the previous frame's rows 0..3 verbatim (they
// already contain the previous effective scale including fade, and the colour/hemisphere row).
struct FoliageItem {
    bool valid{};
    float scale{}, fade{};
    float rows[16]{};
};
// previous_clip is the unjittered previous projection * view (column-vector, mul(M, v)); xform holds the three float4
// registers of the engine's m_xform exactly as the engine staged them. Both return false and write an all-invalid buffer
// when a value is not finite.
bool pack_foliage_tree(const temporal::Mat4& previous_clip, const float (&xform)[12], const FoliageConstants& previous,
    FoliageVertexBytes& out) noexcept;
// `groups` is the number of live groups in the current batch (at most 61); items[g] is the remap of current group g.
bool pack_foliage_detail(const temporal::Mat4& previous_clip, const FoliageConstants& previous,
    std::span<const FoliageItem> items, std::uint32_t groups, FoliageVertexBytes& out) noexcept;
// A projection that carries sub-pixel jitter (normalized viewport UV, Y down, added to clip.xy / clip.w as
// (2u, -2v)) without it: P' = P with row0 -= 2u * row3 and row1 += 2v * row3. The foliage shader contract wants an
// unjittered previous projection.
temporal::Mat4 remove_jitter(const temporal::Mat4& projection, float jitter_u, float jitter_v) noexcept;

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
