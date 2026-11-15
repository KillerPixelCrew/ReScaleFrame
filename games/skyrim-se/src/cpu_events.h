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
bool cpu_render_identity(rsf_game_render_pass&);
// Read only the native event headers while the dispatch callback owns the list.
uint32_t cpu_input_kinds(const void* list) noexcept;
}
