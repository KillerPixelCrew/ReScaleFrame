// SPDX-License-Identifier: MIT
#pragma once
#include <windows.h>

// The carrier owns the Runtime reference. These calls only borrow it.
extern "C" HMODULE rsf_anomaly_runtime_module() noexcept;
extern "C" FARPROC rsf_anomaly_runtime_symbol(HMODULE, const char*) noexcept;
extern "C" void rsf_anomaly_runtime_set_colour_transport(bool) noexcept;
