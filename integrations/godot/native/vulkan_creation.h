// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "api.h"
namespace rsf::godot_vk {
rsf_godot_vk_proc creation_resolver(rsf_godot_vk_proc base, bool xess, rsf_game_log_fn log, void* user);
bool xess_device_ready();
bool interop_device_ready();
void stop_creation_requirements();
}
