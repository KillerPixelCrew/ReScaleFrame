// SPDX-License-Identifier: MIT
#pragma once
#include <d3d12.h>
#include <wrl/client.h>
#include "package_state.h"
void rsf_unity_package_bridge_error(int32_t result, const char* message) noexcept;
// Acquires the active bridge's device and queue with owning references.
bool rsf_unity_package_graphics(Microsoft::WRL::ComPtr<ID3D12Device>& device,
    Microsoft::WRL::ComPtr<ID3D12CommandQueue>& queue) noexcept;
bool rsf_unity_package_is_editor() noexcept;
bool rsf_unity_package_bridge_open(bool editor);
bool rsf_unity_package_bridge_try_close() noexcept;
void rsf_unity_package_bridge_finish_retirement(bool commit) noexcept;
