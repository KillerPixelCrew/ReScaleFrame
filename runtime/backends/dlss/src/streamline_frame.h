// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/streamline_host.h>
#if RSF_HAVE_STREAMLINE
#include <sl.h>
#include <sl_dlss.h>
// Internal to this backend module, never part of the public DLL or game ABI.
bool rsf_streamline_common_set(rsf_streamline_host*, uint64_t, uint32_t, const sl::Constants&);
namespace rsf {
// Row major on both sides, so a copy rather than a transpose.
inline sl::float4x4 sl_matrix(const float* values)
{
    sl::float4x4 matrix{};
    for (int row = 0; row < 4; ++row) {
        matrix.row[row].x = values[row * 4 + 0];
        matrix.row[row].y = values[row * 4 + 1];
        matrix.row[row].z = values[row * 4 + 2];
        matrix.row[row].w = values[row * 4 + 3];
    }
    return matrix;
}
// rsf_quality and rsf_dlss_quality share their values.
inline sl::DLSSMode dlss_mode(uint32_t quality)
{
    switch (quality) {
    case 0: return sl::DLSSMode::eDLAA;
    case 1: return sl::DLSSMode::eMaxQuality;
    case 2: return sl::DLSSMode::eBalanced;
    case 3: return sl::DLSSMode::eMaxPerformance;
    case 4: return sl::DLSSMode::eUltraPerformance;
    default: return sl::DLSSMode::eOff;
    }
}
}
#endif
