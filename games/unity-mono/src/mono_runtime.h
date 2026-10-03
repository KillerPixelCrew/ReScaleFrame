// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/unity_bridge.h>

bool rsf_unity_mono_start(const wchar_t* helper, rsf_unity_native_api* api, const char** reason) noexcept;
bool rsf_unity_mono_stop() noexcept;
