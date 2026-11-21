// SPDX-License-Identifier: MIT
#pragma once
#include "fallout4_platform.h"

namespace rsf::fallout4 {
// Exact build identity and the renderer owner's window. The Rust owner (lib.rs) implements both;
// the render and CPU guards keep calling them by these names.
inline bool verify_executable() noexcept { return rsf_fallout4_verify_executable(); }
inline int accept_window(void* user, void* window) noexcept { return rsf_fallout4_accept_window(user, window); }
// Native leaves of the Rust creation owner (creation.rs): the hook on the creation import thunk,
// whose detour forwards its arguments and return address, and the call through its trampoline.
bool create_creation_hook(void* target) noexcept;
int32_t call_creation(const rsf_fallout4_create_call*) noexcept;
}
