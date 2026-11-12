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
