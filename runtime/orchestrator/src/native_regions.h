// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <d3d11.h>
#include <cstdint>
struct rsf_native_regions;
// Region-local inputs: linear RGB becomes RGBA16_FLOAT, depth R32_FLOAT; packed motion stays raw.
bool rsf_native_regions_prepare(ID3D11DeviceContext*, ID3D11Texture2D* color, ID3D11Texture2D* depth,
    ID3D11Texture2D* motion, const int32_t rect[4], ID3D11Texture2D** out_color,
    ID3D11Texture2D** out_depth, ID3D11Texture2D** out_motion);
void rsf_native_regions_release();
