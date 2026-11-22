/* SPDX-License-Identifier: MIT */
#include "ac7_scope_owner.h"

int rsf_ac7_render_scopes_create(uint64_t session, uint32_t capacity,
    rsf_ac7_render_scope_fn callback, void* user, rsf_ac7_render_scopes** out)
{
    return rsf_ac7_scope_owner_create(session, capacity, callback, NULL, user,
        &rsf_ac7_scope_exception_calls, out);
}

int rsf_ac7_render_scopes_create_passes(uint64_t session, uint32_t capacity,
    rsf_ac7_render_scope_fn state_callback, rsf_ac7_render_scope_fn pass_callback,
    void* user, rsf_ac7_render_scopes** out)
{
    return rsf_ac7_scope_owner_create(session, capacity, state_callback, pass_callback, user,
        &rsf_ac7_scope_exception_calls, out);
}
