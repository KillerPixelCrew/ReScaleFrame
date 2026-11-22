// SPDX-License-Identifier: MIT
#pragma once
#include <windows.h>
#include <d3d12.h>
#include <cstdint>

extern "C" int32_t rsf_core_d3d12_wait_native_fence(void*, uint64_t, void*, uint32_t);

namespace rsf {
// The caller owns the auto-reset event. The Rust wait retains the exact completion value and HRESULT.
inline HRESULT wait_fence(ID3D12Fence* fence, uint64_t value, HANDLE event, DWORD timeout_ms = 10000)
{
    return static_cast<HRESULT>(rsf_core_d3d12_wait_native_fence(fence, value, event, timeout_ms));
}
}
