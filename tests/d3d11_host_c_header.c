/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/d3d11_host.h>
#include <stddef.h>
_Static_assert(sizeof(rsf_d3d11_reduced_frame) == 56, "Recovery request size");
_Static_assert(offsetof(rsf_d3d11_reduced_frame, token) == 16, "Token offset");
_Static_assert(offsetof(rsf_d3d11_reduced_frame, owner) == 24, "Owner offset");
_Static_assert(offsetof(rsf_d3d11_reduced_frame, surfaces) == 40, "Surfaces offset");
_Static_assert(offsetof(rsf_d3d11_reduced_frame, temporal_result) == 48, "Result offset");
/* Public negotiation remains consumable as C without graphics or C++ headers. */
uint32_t rsf_reduced_frame_c_header_check(void) { return RSF_D3D11_REDUCED_FRAME_ABI_VERSION; }
