// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/backend.h>
#include <windows.h>
#include <d3d12.h>
#include <memory>

/** Opaque lease for the private FSR4 capability override, holding the device alive. */
struct Fsr4Compatibility;
// Private, module-version-guarded INT8 enablement. Empty means unavailable/refused, with native
// capability queries retained. The supplied HMODULE remains caller-owned. The lease must outlive the FFX
// context. Code/trampoline modules stay resident; dropping the last lease restores native
// capability answers. No adapter information is changed outside the SDK.
std::shared_ptr<Fsr4Compatibility> rsf_fsr4_enable_int8(HMODULE module, ID3D12Device* device,
    rsf_backend_log_fn log, void* user);
