// SPDX-License-Identifier: MIT
#pragma once
#include "api.h"
// Forwarders to the creation owner in native/vulkan/creation.rs (godot_platform.h).
namespace rsf::godot_vk {
// The resolver to hand the engine, or base unchanged without Vulkan SR. Null without base.
rsf_godot_vk_proc creation_resolver(rsf_godot_vk_proc base, bool xess, rsf_game_log_fn log, void* user);
bool xess_device_ready();
bool interop_device_ready();
void stop_creation_requirements();
}
