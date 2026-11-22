/* SPDX-License-Identifier: MIT */
/* Native leaves of the Rust Streamline callers. Each crate that calls the SDK
   compiles its own copy; RSF_SLVK_GUARDED names it. */
#include <stdint.h>

#ifndef RSF_SLVK_GUARDED
#define RSF_SLVK_GUARDED rsf_slvk_guarded
#endif

typedef int32_t (*rsf_slvk_guarded_call)(void*);

#if defined(_MSC_VER)
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

/* The Rust thunk and its SDK function pointers use the C-unwind ABI, so a
   foreign C++ exception (SEH code 0xE06D7363) from the SDK unwinds to this
   frame and becomes the caller's result. Other faults propagate. */
int32_t RSF_SLVK_GUARDED(rsf_slvk_guarded_call call, void* user, int32_t thrown)
{
    __try {
        return call(user);
    } __except (GetExceptionCode() == 0xE06D7363UL ? EXCEPTION_EXECUTE_HANDLER : EXCEPTION_CONTINUE_SEARCH) {
        return thrown;
    }
}
#else
/* The MinGW standalone bridge has no structured exception handling. The
   Streamline interposer returns eErrorExceptionHandler from its own SEH guard
   for exceptions inside its API. */
int32_t RSF_SLVK_GUARDED(rsf_slvk_guarded_call call, void* user, int32_t thrown)
{
    (void)thrown;
    return call(user);
}
#endif

#if !defined(RSF_SLVK_GUARD_ONLY)
/* sl::FrameToken has one virtual member, operator uint32_t, in vtable slot 0.
   x64 member calls pass the object as the first argument. */
typedef uint32_t (*rsf_slvk_token_index_fn)(const void*);
uint32_t rsf_slvk_token_index(const void* token)
{
    const rsf_slvk_token_index_fn* table = *(const rsf_slvk_token_index_fn* const*)token;
    return table[0](token);
}
#endif
