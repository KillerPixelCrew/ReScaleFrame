// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/unity_bridge.h>

bool rsf_unity_bridge_prepare(const rsf_game_host_services& host, rsf_unity_native_api* api) noexcept;
void rsf_unity_bridge_activate(bool enabled) noexcept;
bool rsf_unity_bridge_drained() noexcept;
void rsf_unity_bridge_release() noexcept;
uint32_t rsf_unity_bridge_managed_state() noexcept;
