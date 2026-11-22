// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
namespace rsf::fallout4 {
// The render hook lifecycle (verify, prepare, start, quiesce, stop, busy) is Rust
// (render_hooks.rs), with its leaves in render_hooks_platform.h.
// The CPU owner calls this after native Swap, before it clears the source
// identity. The Rust render scope balances SUBMISSION, WINDOW and FRAME, then
// tries to retire recovery, auxiliary and resolution ownership.
void finish_render_scope() noexcept;
}
