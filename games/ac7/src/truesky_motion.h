// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <d3d11.h>
#include <cstdint>

// Device depth of the dominant cloud surface at each scene pixel, for reprojecting pixels that
// have no object velocity. Written at TrueSky's composite_tile draw while its inputs are bound.
struct rsf_ac7_cloud_depth {
    ID3D11Device* device = nullptr;
    ID3D11ComputeShader* shader = nullptr;
    ID3D11Buffer* constants = nullptr;
    ID3D11Texture2D* texture = nullptr;
    ID3D11UnorderedAccessView* view = nullptr;
    uint32_t width = 0, height = 0;
    bool refused = false;
};
// Reads the composite's bound far image (t1, alpha = transmittance), near/far/cloud distance
// (t2, z = km to the first 25% opacity) and HdrConstants (b12), and writes an R32_FLOAT texture
// of the viewport. Zero where no sufficiently opaque cloud lies in front. Restores compute state.
// Returns the texture (owned by state) or null when the bindings do not match the measured ABI.
ID3D11Texture2D* rsf_ac7_cloud_depth_write(rsf_ac7_cloud_depth& state, ID3D11DeviceContext* context,
    const D3D11_VIEWPORT& viewport);
void rsf_ac7_cloud_depth_release(rsf_ac7_cloud_depth& state);
