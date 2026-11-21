// SPDX-License-Identifier: MIT
#pragma once
#include <d3d12.h>
#include <stdint.h>
#include <wrl/client.h>
#include <rescaleframe/unity_bridge.h>
// The managed contract structures and the Rust state boundary.
#include "package_state.h"
void rsf_unity_package_bridge_error(int32_t result, const char* message) noexcept;
bool rsf_unity_package_graphics(Microsoft::WRL::ComPtr<ID3D12Device>& device,
    Microsoft::WRL::ComPtr<ID3D12CommandQueue>& queue) noexcept;
bool rsf_unity_package_is_editor() noexcept;
bool rsf_unity_package_bridge_open(bool editor);
bool rsf_unity_package_bridge_try_close() noexcept;
void rsf_unity_package_bridge_finish_retirement(bool commit) noexcept;
#define RSF_UNITY_PREVIEW_SOURCE_COMMON 0x40000000u
