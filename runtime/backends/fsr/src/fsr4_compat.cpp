// SPDX-License-Identifier: MIT
#include "fsr4_compat.h"
#include "fsr4_compat_core.h"
#include <wrl/client.h>

const rsf_fsr4_compat_lease* rsf_fsr4_acquire_int8(HMODULE module, ID3D12Device* device,
    rsf_backend_log_fn log, void* user) try
{
    if (!module || !device) return {};
    Microsoft::WRL::ComPtr<IDXGIFactory4> factory;
    Microsoft::WRL::ComPtr<IDXGIAdapter1> adapter;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) ||
        FAILED(factory->EnumAdapterByLuid(device->GetAdapterLuid(), IID_PPV_ARGS(&adapter)))) return {};
    return rsf_fsr4_acquire_int8_adapter(module, adapter.Get(), device, log, user);
}
catch (...) {
    if (log) log(user, "FSR4 INT8 compatibility adapter lookup failed");
    return {};
}
