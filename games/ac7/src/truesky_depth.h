// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <d3d11.h>

// Creates the missing one-to-one depth-bounds kernel for the measured TrueSky binding ABI.
// device and shader are required; valid calls clear *shader before preparation. Returns true
// with one owned shader reference, false on compiler/device refusal. The native effect owns
// CB/SRV/UAV binding and cleanup; creation does not install hooks or activate full-grid rendering.
bool rsf_ac7_create_truesky_depth_shader(ID3D11Device* device, ID3D11ComputeShader** shader);
