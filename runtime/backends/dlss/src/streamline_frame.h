// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/streamline_host.h>
#if RSF_HAVE_STREAMLINE
#include <sl.h>
#include <sl_dlss.h>
// Internal to this backend module, never part of the public DLL or game ABI.
bool rsf_streamline_common_set(rsf_streamline_host*, uint64_t, uint32_t, const sl::Constants&);
extern "C" void rsf_streamline_camera_forward(const float*, const float*, float, float*);
namespace rsf {
// Perspective clip W encodes the view-space forward sign. Preserve each caller's existing
// orthographic behavior when W is independent of Z; no engine identity is inferred here.
inline sl::float3 sl_camera_forward(const rsf_camera_frame& camera, float orthographic_sign)
{
    float forward[3]{};
    rsf_streamline_camera_forward(camera.view_to_world, camera.view_to_clip, orthographic_sign, forward);
    return {forward[0], forward[1], forward[2]};
}
}
#endif
