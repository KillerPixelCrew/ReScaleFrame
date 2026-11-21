// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_renderer.h>

namespace rsf::wingman {
bool create_execution_hook(uint32_t index, uintptr_t target) noexcept;
void report_execution(const rsf_game_host_services*, void*, const rsf_game_render_pass*) noexcept;
}
