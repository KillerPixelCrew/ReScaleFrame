// SPDX-License-Identifier: MIT
#include <d3d12.h>
#include <cstdint>

extern "C" int32_t rsf_fg_lease_com_get_device(void* child, void** out)
{
    // Both resource and fence interfaces inherit the ID3D12DeviceChild prefix.
    ID3D12Device* device = nullptr;
    const HRESULT result = static_cast<ID3D12DeviceChild*>(child)->GetDevice(IID_PPV_ARGS(&device));
    *out = device;
    return result;
}

extern "C" uint64_t rsf_d3d12_fence_completed(void* fence)
{
    return static_cast<ID3D12Fence*>(fence)->GetCompletedValue();
}
