// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/depth_scale.h>
#include <d3d11.h>
// Internal allocation-only readiness; same primitive used by public prepare.
bool rsf_depth_scale_reserve(rsf_depth_scale*, ID3D11Texture2D*);
