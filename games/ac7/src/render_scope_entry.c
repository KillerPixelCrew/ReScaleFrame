/* SPDX-License-Identifier: MIT */
/* The two C create calls and the consumer-call leaves they pass to the Rust scope owner.
   Each leaf contains a C++ exception thrown by an observer or lease before execution returns to
   Rust or the engine's command iterator. MSVC raises a C++ exception as SEH code 0xE06D7363, so a
   filter on that code stops the same exceptions the former catch-all stopped under /EHsc. Access
   violations and other faults still propagate. */
#include "ac7_scope_owner.h"

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#define RSF_CXX_EXCEPTION_CODE 0xE06D7363UL

#if defined(_MSC_VER)
#define RSF_FOREIGN_TRY __try
#define RSF_FOREIGN_CATCH(on_exception) \
    __except (GetExceptionCode() == RSF_CXX_EXCEPTION_CODE ? EXCEPTION_EXECUTE_HANDLER \
                                                           : EXCEPTION_CONTINUE_SEARCH) { on_exception; }
#else
#error "Foreign C++ exception containment needs MSVC structured exception handling"
#endif

/* Observers cannot interrupt native work or ticket retirement. */
static bool notify(rsf_ac7_render_scope_fn callback, void* user, void* command_list,
    const rsf_ac7_render_scope* scope, uint32_t begin)
{
    RSF_FOREIGN_TRY {
        callback(user, command_list, scope, begin);
        return true;
    } RSF_FOREIGN_CATCH(return false)
}

static bool resolve(const rsf_ac7_scope_lease* lease, rsf_ac7_render_scope* scope)
{
    RSF_FOREIGN_TRY {
        lease->resolve(lease->object, scope);
        return true;
    } RSF_FOREIGN_CATCH(return false)
}

static bool release(const rsf_ac7_scope_lease* lease)
{
    RSF_FOREIGN_TRY {
        lease->release(lease->object);
        return true;
    } RSF_FOREIGN_CATCH(return false)
}

const rsf_ac7_scope_calls rsf_ac7_scope_exception_calls = { notify, resolve, release };

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
