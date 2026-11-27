/* SPDX-License-Identifier: MIT */
#include "ac7_render_scope_fixture.h"

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include <rescaleframe/foreign_guard.h>
#define RSF_CXX_EXCEPTION_MAGIC 0x19930520UL

/* C cannot throw a C++ object, so the deliberate foreign exceptions raise the MSVC C++ exception
   code directly. The scope owner's leaves filter on that code, so this exercises the same
   containment path as a thrown object. */
static void raise_foreign_exception(void)
{
    ULONG_PTR parameters[4] = { RSF_CXX_EXCEPTION_MAGIC, 0, 0, 0 };
    RaiseException(RSF_CXX_EXCEPTION_CODE, EXCEPTION_NONCONTINUABLE, 4, parameters);
}

void rsf_ac7_fixture_throwing_pass(void* user, void* list, const rsf_ac7_render_scope* scope,
    uint32_t begin)
{
    (void)user; (void)list; (void)scope; (void)begin;
    raise_foreign_exception();
}

void rsf_ac7_fixture_throwing_resolve(void* object, rsf_ac7_render_scope* scope)
{
    rsf_ac7_fixture_lease* lease = (rsf_ac7_fixture_lease*)object;
    ++lease->resolves;
    if (lease->throw_resolve) raise_foreign_exception();
    scope->native_frame += 100;
}

void rsf_ac7_fixture_throwing_release(void* object)
{
    ++((rsf_ac7_fixture_lease*)object)->releases;
    raise_foreign_exception();
}

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
