// SPDX-License-Identifier: MIT
#pragma once
#include <d3d12.h>
#include <wrl/client.h>
#include "package_state.h"
void rsf_unity_package_bridge_error(int32_t result, const char* message) noexcept;
bool rsf_unity_package_is_editor() noexcept;
