// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/unity_bridge.h>

// Copy host services and publish the managed callback table. Lifecycle serialization is required;
// callback/user storage remains borrowed until all queued packets and commands drain.
bool rsf_unity_bridge_prepare(const rsf_game_host_services& host, rsf_unity_native_api* api) noexcept;
// Gate enqueue/configuration admission. Existing event-data pointers still require consumption.
void rsf_unity_bridge_activate(bool enabled) noexcept;
// True only after callbacks, pending events and both GPU command rings complete. An unknown
// submission completion permanently refuses release for this owned session.
bool rsf_unity_bridge_drained() noexcept;
// Release COM storage and borrowed services after the caller establishes drained state.
void rsf_unity_bridge_release() noexcept;
// Latest managed state report: see unity_bridge.h. This does not prove rendering acceptance.
uint32_t rsf_unity_bridge_managed_state() noexcept;
