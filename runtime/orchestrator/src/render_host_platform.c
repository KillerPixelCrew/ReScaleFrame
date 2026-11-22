/* SPDX-License-Identifier: MIT */
#include "render_host_foreign.h"

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <rescaleframe/foreign_guard.h>

#if !defined(_MSC_VER)
#error "Foreign C++ exception containment needs MSVC structured exception handling"
#endif

/* The engine owns the Streamline DLL. Keep any foreign C++ exception (SEH code 0xE06D7363) on
   this side of the Rust call and let the host retain the original interface. Other faults
   propagate as before. */
int32_t rsf_render_host_call_native_interface(rsf_render_host_native_interface_fn entry, void* proxy, void** native)
{
    RSF_FOREIGN_TRY {
        return entry(proxy, native);
    } RSF_FOREIGN_CATCH {
        return -1;
    }
}
