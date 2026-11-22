// SPDX-License-Identifier: MIT
#include "fsr4_compat_core.h"
#include <utility>

namespace {
using NativeLease = std::unique_ptr<const rsf_fsr4_compat_lease, decltype(&rsf_fsr4_compat_release)>;
}

struct Fsr4Compatibility {
    NativeLease lease;
    explicit Fsr4Compatibility(NativeLease value) : lease(std::move(value)) {}
};

std::shared_ptr<Fsr4Compatibility> rsf_fsr4_enable_int8_adapter(HMODULE module,
    IDXGIAdapter1* adapter, ID3D12Device* device, Fsr4Log log, void* user) try
{
    NativeLease lease(rsf_fsr4_compat_acquire_native_adapter(module, adapter, device, log, user),
        rsf_fsr4_compat_release);
    if (!lease) return {};
    return std::make_shared<Fsr4Compatibility>(std::move(lease));
}
catch (...) {
    if (log) log(user, "FSR4 INT8 compatibility preparation failed");
    return {};
}
