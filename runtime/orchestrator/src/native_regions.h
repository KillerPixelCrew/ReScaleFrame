// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <d3d11.h>
#include <cstdint>
struct rsf_native_regions;
// Region-local inputs: RGB becomes RGBA16_FLOAT without a transfer function,
// depth R32_FLOAT; packed motion stays raw. Colour space remains an adapter fact.
bool rsf_native_regions_prepare(ID3D11DeviceContext*, ID3D11Texture2D* color, ID3D11Texture2D* depth,
    ID3D11Texture2D* motion, const int32_t rect[4], ID3D11Texture2D** out_color,
    ID3D11Texture2D** out_depth, ID3D11Texture2D** out_motion);
// Optional numeric temporal mask. Both outputs are render-local R32_FLOAT, composed
// by maximum with existing render-local scalar hints. Caller restores graphics bindings.
bool rsf_native_regions_temporal_mask(ID3D11DeviceContext*, ID3D11Texture2D* mask,
    const int32_t rect[4], const float weights[4], ID3D11Texture2D* reactive, ID3D11Texture2D* bias,
    ID3D11Texture2D** out_reactive, ID3D11Texture2D** out_bias);
void rsf_native_regions_release();
