// SPDX-License-Identifier: MIT
// Callback exceptions are contained here before execution returns to the Rust owner.
#include "ac7_scope_owner.h"

namespace {
bool notify(rsf_ac7_render_scope_fn callback, void* user, void* command_list,
    const rsf_ac7_render_scope* scope, uint32_t begin) noexcept
{
    try { callback(user, command_list, scope, begin); return true; }
    catch (...) { return false; } // Observers cannot interrupt native work or ticket retirement.
}
bool resolve(const rsf_ac7_scope_lease* lease, rsf_ac7_render_scope* scope) noexcept
{
    try { lease->resolve(lease->object, scope); return true; }
    catch (...) { return false; }
}
bool release(const rsf_ac7_scope_lease* lease) noexcept
{
    try { lease->release(lease->object); return true; }
    catch (...) { return false; }
}
}

extern "C" const rsf_ac7_scope_calls rsf_ac7_scope_exception_calls{notify, resolve, release};
