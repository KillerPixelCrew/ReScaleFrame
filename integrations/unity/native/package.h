// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>
#include <rescaleframe/unity_bridge.h>
// The managed contract structures and the Rust state boundary.
#include "package_state.h"
void rsf_unity_package_bridge_error(int32_t result, const char* message) noexcept;
bool rsf_unity_package_graphics(void** device, void** queue) noexcept;
bool rsf_unity_package_is_editor() noexcept;
#define RSF_UNITY_PREVIEW_SOURCE_COMMON 0x40000000u
