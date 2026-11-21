// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/streamline_host.h>
#if RSF_HAVE_STREAMLINE
#include <sl.h>
#include <sl_dlss.h>
// Internal to this backend module, never part of the public DLL or game ABI.
bool rsf_streamline_common_set(rsf_streamline_host*, uint64_t, uint32_t, const sl::Constants&);
namespace rsf {
// Perspective clip W encodes the view-space forward sign. Preserve each caller's existing
// orthographic behavior when W is independent of Z; no engine identity is inferred here.
inline sl::float3 sl_camera_forward(const rsf_camera_frame& camera,float orthographic_sign)
{
    const auto w= camera.view_to_clip[11];
    const float sign=w<0?-1.0f:(w>0?1.0f:orthographic_sign);
    return {sign*camera.view_to_world[8],sign*camera.view_to_world[9],sign*camera.view_to_world[10]};
}
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
}
#endif
