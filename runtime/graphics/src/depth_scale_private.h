// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/depth_scale.h>
#include <d3d11.h>
// Internal allocation-only readiness; same primitive used by public prepare.
bool rsf_depth_scale_reserve(rsf_depth_scale*, ID3D11Texture2D*);
// Rebuild a private product from the already sealed original, without resnapshotting.
int32_t rsf_depth_scale_redraw(rsf_depth_scale*, const rsf_depth_scale_frame*);
