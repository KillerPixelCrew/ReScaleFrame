/* SPDX-License-Identifier: MIT */
#include "native_latency_vulkan_platform.h"

/* A borrowed callback can enter foreign C++ code, and an exception that unwinds into the Rust
   lease is undefined. MSVC raises a C++ throw as structured exception 0xE06D7363, which this leaf
   contains. It leaves every other fault alone. Other toolchains call the callback directly. */
#if defined(_MSC_VER)
#include <windows.h>

#define RSF_FOREIGN_CXX_EXCEPTION 0xE06D7363UL
static int foreign_exception(unsigned long code)
{
    return code == RSF_FOREIGN_CXX_EXCEPTION ? EXCEPTION_EXECUTE_HANDLER : EXCEPTION_CONTINUE_SEARCH;
}
#endif

int32_t rsf_sllatency_invoke_checked(rsf_slvk_callback callback, void* module, uintptr_t token,
                                     void* user)
{
#if defined(_MSC_VER)
    __try {
#endif
        return callback(module, token, user);
#if defined(_MSC_VER)
    } __except (foreign_exception(GetExceptionCode())) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
#endif
}
