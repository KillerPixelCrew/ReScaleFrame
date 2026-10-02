// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <d3d11.h>

// Creates the missing one-to-one depth-bounds kernel for the measured TrueSky binding ABI.
// Returned shader owns one reference. The native effect owns CB/SRV/UAV binding and cleanup.
bool rsf_ac7_create_truesky_depth_shader(ID3D11Device* device, ID3D11ComputeShader** shader);
