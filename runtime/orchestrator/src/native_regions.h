// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <d3d11.h>
#include <cstdint>
struct rsf_native_regions;
// Private D3D11 execution-owner cache. rect is [left,top,right,bottom) in source pixels.
// Region-local inputs: linear RGB becomes RGBA16_FLOAT with alpha one, depth R32_FLOAT;
// packed motion stays raw. Outputs are borrowed until the next prepare/release. All source
// resources must be usable on the context's device; caller saves/restores bindings because
// prepare changes OM/compute state. False does not publish new output pointers.
bool rsf_native_regions_prepare(ID3D11DeviceContext*, ID3D11Texture2D* color, ID3D11Texture2D* depth,
    ID3D11Texture2D* motion, const int32_t rect[4], ID3D11Texture2D** out_color,
    ID3D11Texture2D** out_depth, ID3D11Texture2D** out_motion);
// Release after borrowers stop on the same graphics owner.
void rsf_native_regions_release();
