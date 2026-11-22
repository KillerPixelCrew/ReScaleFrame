// SPDX-License-Identifier: MIT
#pragma once
#include "fallout4_platform.h"

namespace rsf::fallout4 {
// The renderer owner window, checked in Rust.
inline int accept_window(void* user, void* window) noexcept { return rsf_fallout4_accept_window(user, window); }
// Native leaves of the Rust creation owner (creation.rs): the hook on the creation import thunk,
// whose detour forwards its arguments and return address, and the call through its trampoline.
int32_t call_creation(const rsf_fallout4_create_call*) noexcept;
}
