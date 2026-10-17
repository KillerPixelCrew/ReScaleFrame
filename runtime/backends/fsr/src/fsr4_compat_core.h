// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <memory>

struct Fsr4Compatibility;
using Fsr4Log = void (*)(void*, const char*);
// The caller has already matched this adapter to the graphics device. The lease outlives
// the FFX context; the resident hook only changes capability answers for the leased device.
std::shared_ptr<Fsr4Compatibility> rsf_fsr4_enable_int8_adapter(HMODULE module,
    IDXGIAdapter1* adapter, ID3D12Device* device, Fsr4Log log, void* user);
