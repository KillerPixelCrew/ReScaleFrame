// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
namespace rsf::fallout4 {
// The render hook lifecycle (verify, prepare, start, quiesce, stop, busy) is Rust
// (render_hooks.rs), with its leaves in render_hooks_platform.h.
// CPU owner calls after the unchanged native Swap (including actual Present),
// before clearing its current identity. Closes WINDOW/FRAME and drains any
// refused SUBMISSION whose completion was withheld before Present.
// Pending auxiliary/native-state ownership blocks the next World snapshot.
void finish_render_scope() noexcept;
}
