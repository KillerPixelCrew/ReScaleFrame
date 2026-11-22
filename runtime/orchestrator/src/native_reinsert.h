// SPDX-License-Identifier: MIT
#pragma once
#include <d3d11.h>
#include <cstdint>

// Immediate-context owner only. Bindings and predication are preserved through the shared scope.
// Source is a rect-local linear reconstruction. Both destinations are preflighted before writes.
// Different floating-point colour formats convert without a transfer function or tonemapping.
extern "C" {
bool rsf_native_reinsert(ID3D11DeviceContext*, ID3D11Texture2D* source,
    ID3D11Texture2D* destination, ID3D11Texture2D* readable, const int32_t rect[4]);
void rsf_native_reinsert_release();
void* rsf_native_scalar_exposure(void* context, void* input);
}
