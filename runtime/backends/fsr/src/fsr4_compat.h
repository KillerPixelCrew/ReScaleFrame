// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/backend.h>
#include <windows.h>
#include <d3d12.h>
#include "fsr4_compat_platform.h"

// Private, module-version-guarded INT8 enablement. The returned lease must outlive the FFX
// context. Code/trampoline modules stay resident; dropping the last lease restores native
// capability answers. No adapter information is changed outside the SDK.
const rsf_fsr4_compat_lease* rsf_fsr4_acquire_int8(HMODULE module, ID3D12Device* device,
    rsf_backend_log_fn log, void* user);
