/* SPDX-License-Identifier: MIT */
#include "ac7_render_scope_fixture.h"

/* Construct the caller's records with the public C definition of the scope ABI. */
int rsf_ac7_fixture_enqueue(rsf_ac7_render_scopes* owner, void* list, rsf_ac7_fixture_lease* lease)
{
    const rsf_ac7_render_scope scope = {
        .struct_size = (uint32_t)sizeof(scope), .session_id = 7, .family_key = 0x100,
        .view_key = 0x200, .native_frame = 5
    };
    const rsf_ac7_scope_lease owned = {
        lease, rsf_ac7_fixture_throwing_resolve, rsf_ac7_fixture_throwing_release
    };
    return rsf_ac7_render_scope_enqueue_leased(owner, list, &scope, &owned);
}

uint64_t rsf_ac7_fixture_native_frame(const rsf_ac7_render_scope* scope)
{
    return scope->native_frame;
}

int rsf_ac7_fixture_current(rsf_ac7_render_scopes* owner)
{
    rsf_ac7_render_scope scope = { .struct_size = (uint32_t)sizeof(scope) };
    return rsf_ac7_render_scope_current(owner, &scope);
}

int main(void)
{
    return rsf_ac7_render_scope_fixture();
}
