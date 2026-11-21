// SPDX-License-Identifier: MIT
// The queued render scopes are owned by render_scope.rs in the rsf-ac7 crate, which exports the
// rest of ac7_render_scope.h. This file keeps the two create calls so it can lend the leaves that
// call consumer code on the engine's RHI stream with every C++ exception contained.
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
constexpr rsf_ac7_scope_calls calls{notify, resolve, release};
}

extern "C" int rsf_ac7_render_scopes_create(uint64_t session, uint32_t capacity,
    rsf_ac7_render_scope_fn callback, void* user, rsf_ac7_render_scopes** out)
{
    return rsf_ac7_scope_owner_create(session, capacity, callback, nullptr, user, &calls, out);
}

extern "C" int rsf_ac7_render_scopes_create_passes(uint64_t session, uint32_t capacity,
    rsf_ac7_render_scope_fn state_callback, rsf_ac7_render_scope_fn pass_callback,
    void* user, rsf_ac7_render_scopes** out)
{
    return rsf_ac7_scope_owner_create(session, capacity, state_callback, pass_callback, user, &calls, out);
}
