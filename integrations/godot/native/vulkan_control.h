// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "api.h"
namespace rsf::godot_vk {
bool booted();
int configure(const rsf_godot_settings*);
int status(rsf_godot_status*);
int settings(rsf_godot_settings*);
int shutdown();
}
