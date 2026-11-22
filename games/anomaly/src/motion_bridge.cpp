// SPDX-License-Identifier: MIT
#include "motion_constants.h"

namespace rsf::anomaly::motion {
static_assert(sizeof(temporal::Mat4) == 128 && sizeof(PixelInputs) == 32 && sizeof(CoreHistory) == 3);
static_assert(sizeof(FoliageConstants) == 48);

extern "C" {
bool rsf_anomaly_motion_pack_core(const temporal::Mat4*, const temporal::Mat4*, std::size_t, const CoreHistory*, std::byte*) noexcept;
void rsf_anomaly_motion_pack_foliage_invalid(std::byte*) noexcept;
bool rsf_anomaly_motion_pack_tree(const temporal::Mat4*, const float*, const FoliageConstants*, std::byte*) noexcept;
bool rsf_anomaly_motion_pack_pixel(const PixelInputs*, std::byte*) noexcept;
void rsf_anomaly_motion_compose(const temporal::Mat4*, const temporal::Mat4*, temporal::Mat4*) noexcept;
}

bool pack_core_vertex(const temporal::Mat4& previous_wvp, std::span<const temporal::Mat4> bones, CoreHistory history,
    CoreVertexBytes& out) noexcept
{
    return rsf_anomaly_motion_pack_core(&previous_wvp, bones.data(), bones.size(), &history, out.data());
}
void pack_foliage_invalid(FoliageVertexBytes& out) noexcept { rsf_anomaly_motion_pack_foliage_invalid(out.data()); }
bool pack_foliage_tree(const temporal::Mat4& previous_clip, const float (&xform)[12], const FoliageConstants& previous,
    FoliageVertexBytes& out) noexcept
{
    return rsf_anomaly_motion_pack_tree(&previous_clip, xform, &previous, out.data());
}
bool pack_pixel(const PixelInputs& in, PixelBytes& out) noexcept { return rsf_anomaly_motion_pack_pixel(&in, out.data()); }

temporal::Mat4 compose(const temporal::Mat4& a, const temporal::Mat4& b) noexcept
{
    temporal::Mat4 out;
    rsf_anomaly_motion_compose(&a, &b, &out);
    return out;
}
}
