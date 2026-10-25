// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "api.h"
namespace rsf::godot_vk {
bool booted();
int configure(const rsf_godot_settings*);
int status(rsf_godot_status*);
int settings(rsf_godot_settings*);
int shutdown();
void cpu(const rsf_game_cpu_event*);
void render_begin(uint64_t);
int input(uint64_t frame, uint32_t kind, uint32_t message);
uint32_t ping_message();
}
