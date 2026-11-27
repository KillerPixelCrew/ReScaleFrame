/* SPDX-License-Identifier: MIT */
/* Native leaves of the Rust Streamline callers in rsf-streamline-vk. */
#include <stdint.h>
#if defined(_MSC_VER)
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#endif
#include <rescaleframe/foreign_guard.h>

typedef int32_t (*rsf_slvk_guarded_call)(void*);

/* The Rust thunk and its SDK function pointers use the C-unwind ABI, so a
   foreign C++ exception from the SDK unwinds to this frame and becomes the
   caller's result. Other faults propagate. The MinGW standalone bridge has no
   structured exception handling (the guard is a plain call there); the
   Streamline interposer returns eErrorExceptionHandler from its own SEH guard
   for exceptions inside its API. */
int32_t rsf_slvk_guarded(rsf_slvk_guarded_call call, void* user, int32_t thrown)
{
    RSF_FOREIGN_TRY {
        return call(user);
    } RSF_FOREIGN_CATCH {
        return thrown;
    }
}

/* sl::FrameToken has one virtual member, operator uint32_t, in vtable slot 0.
   x64 member calls pass the object as the first argument. */
typedef uint32_t (*rsf_slvk_token_index_fn)(const void*);
uint32_t rsf_slvk_token_index(const void* token)
{
    const rsf_slvk_token_index_fn* table = *(const rsf_slvk_token_index_fn* const*)token;
    return table[0](token);
}
