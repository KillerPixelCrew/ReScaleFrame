// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
namespace rsf::fallout4 {
bool verify_render_hooks() noexcept;
bool prepare_render_hooks(const rsf_game_host_services&);
bool start_render_hooks();
void quiesce_render_hooks();
bool stop_render_hooks();
bool render_hooks_busy();
// CPU owner calls after the unchanged native Swap (including actual Present),
// before clearing its current identity. Closes WINDOW/FRAME and drains any
// refused SUBMISSION whose completion was withheld before Present.
// Pending auxiliary/native-state ownership blocks the next World snapshot.
void finish_render_scope() noexcept;
}
