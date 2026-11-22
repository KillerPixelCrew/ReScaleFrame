/* SPDX-License-Identifier: MIT */
#include "frame_tap_platform.h"

/* Frame-tap callbacks can be supplied by C++ clients, and an exception that unwinds into the Rust
   frame tap is undefined. MSVC raises a C++ throw as structured exception 0xE06D7363, so this C leaf
   contains it with a filter that leaves every other fault (access violations included) alone. A
   callback that throws is skipped. Other toolchains call the callback directly. */
#if defined(_MSC_VER)
#include <windows.h>

#define RSF_FOREIGN_CXX_EXCEPTION 0xE06D7363UL
static int foreign_exception(unsigned long code)
{
    return code == RSF_FOREIGN_CXX_EXCEPTION ? EXCEPTION_EXECUTE_HANDLER : EXCEPTION_CONTINUE_SEARCH;
}
#define CONTAIN_TRY __try
#define CONTAIN_CATCH __except (foreign_exception(GetExceptionCode()))
#else
#define CONTAIN_TRY if (1)
#define CONTAIN_CATCH else
#endif

static void call_report(rsf_frame_tap_target_fn fn, void* user, const rsf_frame_tap_target_draw* draw)
{
    CONTAIN_TRY { fn(user, draw); }
    CONTAIN_CATCH {}
}

static void call_geometry(rsf_frame_tap_geometry_fn fn, void* user,
                          const rsf_frame_tap_geometry* geometry)
{
    CONTAIN_TRY { fn(user, geometry); }
    CONTAIN_CATCH {}
}

static void call_compute(rsf_frame_tap_compute_fn fn, void* user, void* context, uint32_t x,
                         uint32_t y, uint32_t z, void* arguments, uint32_t offset)
{
    CONTAIN_TRY { fn(user, context, x, y, z, arguments, offset); }
    CONTAIN_CATCH {}
}

static const rsf_frame_tap_platform callbacks = {
    .report = call_report,
    .geometry = call_geometry,
    .compute = call_compute,
};

const rsf_frame_tap_platform* rsf_frame_tap_foreign_callbacks(void)
{
    return &callbacks;
}

void rsf_frame_tap_call_constant_observer(rsf_frame_tap_constant_observer_fn fn, void* user,
                                          void* buffer, const void* contents, uint32_t bytes)
{
    CONTAIN_TRY { fn(user, buffer, contents, bytes); }
    CONTAIN_CATCH {}
}
