/* SPDX-License-Identifier: MIT */
#pragma once
#include <d3d11.h>

namespace rsf {
extern "C" bool rsf_d3d11_same_device(ID3D11DeviceChild* object, ID3D11Device* device);

/* Keep the existing typed call sites while the ownership check lives in Rust. */
inline constexpr auto same_device = rsf_d3d11_same_device;
}
