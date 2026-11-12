// SPDX-License-Identifier: GPL-3.0-only
#include "motion_constants.h"
#include <cmath>
#include <cstring>

namespace rsf::anomaly::motion {
namespace {
void put(std::byte* out, std::size_t offset, float value) noexcept { std::memcpy(out + offset, &value, sizeof(value)); }
bool usable(const temporal::Mat4& m) noexcept
{
    for (const auto value : m.values)
        if (!std::isfinite(value) || std::abs(value) > 3.0e38) return false;
    return true;
}
void put_rows(std::byte* out, std::size_t offset, const temporal::Mat4& m, std::size_t rows) noexcept
{
    for (std::size_t r = 0; r < rows; ++r)
        for (std::size_t c = 0; c < 4; ++c) put(out, offset + (r * 4 + c) * sizeof(float), static_cast<float>(m.values[r * 4 + c]));
}
}

void pack_core_invalid(CoreVertexBytes& out) noexcept
{
    out.fill(std::byte{});
    put(out.data(), core_history_offset + 12, abi_version);
}

bool pack_core_vertex(const temporal::Mat4& previous_wvp, std::span<const temporal::Mat4> bones, CoreHistory history,
    CoreVertexBytes& out) noexcept
{
    pack_core_invalid(out);
    if (!usable(previous_wvp) || bones.size() > core_max_bones) return false;
    for (const auto& bone : bones)
        if (!usable(bone)) return false;
    put_rows(out.data(), core_wvp_offset, previous_wvp, 4);
    for (std::size_t i = 0; i < bones.size(); ++i)
        put_rows(out.data(), core_bones_offset + i * 3 * 16, bones[i], 3);
    put(out.data(), core_history_offset + 0, history.draw ? 1.0f : 0.0f);
    put(out.data(), core_history_offset + 4, history.palette ? 1.0f : 0.0f);
    put(out.data(), core_history_offset + 8, history.rigid ? 1.0f : 0.0f);
    return true;
}

void pack_foliage_invalid(FoliageVertexBytes& out) noexcept { out.fill(std::byte{}); }

namespace {
void put_u32(std::byte* out, std::size_t offset, std::uint32_t value) noexcept { std::memcpy(out + offset, &value, sizeof(value)); }
bool finite_all(const float* values, std::size_t count) noexcept
{
    for (std::size_t i = 0; i < count; ++i)
        if (!std::isfinite(values[i])) return false;
    return true;
}
bool constants_usable(const FoliageConstants& c) noexcept
{
    return finite_all(c.consts, 4) && finite_all(c.wave, 4) && finite_all(c.wind, 4);
}
void put_floats(std::byte* out, std::size_t offset, const float* values, std::size_t count) noexcept
{
    std::memcpy(out + offset, values, count * sizeof(float));
}
void put_foliage_common(FoliageVertexBytes& out, const temporal::Mat4& clip, const FoliageConstants& c,
    std::uint32_t groups) noexcept
{
    put_rows(out.data(), foliage_clip_offset, clip, 4);
    put_floats(out.data(), foliage_consts_offset, c.consts, 4);
    put_floats(out.data(), foliage_wave_offset, c.wave, 4);
    put_floats(out.data(), foliage_wind_offset, c.wind, 4);
    put_u32(out.data(), foliage_history_offset + 0, 1);       // complete draw history
    put_u32(out.data(), foliage_history_offset + 4, groups);  // detail group count
    put_u32(out.data(), foliage_history_offset + 8, 1);       // ABI 1
}
}

bool pack_foliage_tree(const temporal::Mat4& previous_clip, const float (&xform)[12], const FoliageConstants& previous,
    FoliageVertexBytes& out) noexcept
{
    pack_foliage_invalid(out);
    if (!usable(previous_clip) || !finite_all(xform, 12) || !constants_usable(previous)) return false;
    put_foliage_common(out, previous_clip, previous, 0);
    put_floats(out.data(), foliage_tree_xform_offset, xform, 12);
    return true;
}

bool pack_foliage_detail(const temporal::Mat4& previous_clip, const FoliageConstants& previous,
    std::span<const FoliageItem> items, std::uint32_t groups, FoliageVertexBytes& out) noexcept
{
    pack_foliage_invalid(out);
    if (!usable(previous_clip) || !constants_usable(previous) || groups > foliage_detail_groups || items.size() < groups)
        return false;
    put_foliage_common(out, previous_clip, previous, groups);
    for (std::uint32_t g = 0; g < groups; ++g) {
        const auto& item = items[g];
        // An unmatched group keeps zero rows and a zero state: the shader rejects it before reading the rows.
        if (!item.valid || !finite_all(item.rows, 16) || !(item.scale > 0) || !std::isfinite(item.scale) ||
            !(item.fade > 0 && item.fade <= 1)) continue;
        put_floats(out.data(), foliage_rows_offset + g * 64, item.rows, 16);
        const float state[4] = {1.0f, item.scale, item.fade, 0.0f};
        put_floats(out.data(), foliage_item_state_offset + g * 16, state, 4);
    }
    return true;
}

temporal::Mat4 remove_jitter(const temporal::Mat4& projection, float jitter_u, float jitter_v) noexcept
{
    temporal::Mat4 m = projection;
    for (std::size_t c = 0; c < 4; ++c) {
        m.values[0 * 4 + c] -= 2.0 * static_cast<double>(jitter_u) * projection.values[3 * 4 + c];
        m.values[1 * 4 + c] += 2.0 * static_cast<double>(jitter_v) * projection.values[3 * 4 + c];
    }
    return m;
}

bool pack_pixel(const PixelInputs& in, PixelBytes& out) noexcept
{
    out.fill(std::byte{});
    const float values[] = {in.viewport_x, in.viewport_y, in.viewport_width, in.viewport_height, in.current_jitter_u,
        in.current_jitter_v, in.previous_jitter_u, in.previous_jitter_v};
    for (const auto value : values)
        if (!std::isfinite(value)) return false;
    if (!(in.viewport_width > 0) || !(in.viewport_height > 0)) return false;
    put(out.data(), ps_viewport_offset + 0, in.viewport_x);
    put(out.data(), ps_viewport_offset + 4, in.viewport_y);
    put(out.data(), ps_viewport_offset + 8, 1.0f / in.viewport_width);
    put(out.data(), ps_viewport_offset + 12, 1.0f / in.viewport_height);
    put(out.data(), ps_jitter_offset + 0, in.current_jitter_u);
    put(out.data(), ps_jitter_offset + 4, in.current_jitter_v);
    put(out.data(), ps_jitter_offset + 8, in.previous_jitter_u);
    put(out.data(), ps_jitter_offset + 12, in.previous_jitter_v);
    put(out.data(), ps_control_offset, abi_version);
    return true;
}

temporal::Mat4 from_engine(const float (&values)[16]) noexcept
{
    temporal::Mat4 m;
    for (std::size_t r = 0; r < 4; ++r)
        for (std::size_t c = 0; c < 4; ++c) m.values[r * 4 + c] = values[c * 4 + r];
    return m;
}

temporal::Mat4 compose(const temporal::Mat4& a, const temporal::Mat4& b) noexcept
{
    temporal::Mat4 m;
    for (std::size_t r = 0; r < 4; ++r)
        for (std::size_t c = 0; c < 4; ++c) {
            double sum = 0;
            for (std::size_t k = 0; k < 4; ++k) sum += a.values[r * 4 + k] * b.values[k * 4 + c];
            m.values[r * 4 + c] = sum;
        }
    return m;
}
}
