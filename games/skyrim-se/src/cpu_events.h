// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_api.h>
#include <cstdint>
namespace rsf::skyrim {
bool cpu_prepare(const rsf_game_host_services&);
bool cpu_start();
void cpu_quiesce();
bool cpu_stop();
bool cpu_busy();
uint64_t cpu_source();
}
