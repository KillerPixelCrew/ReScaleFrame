/* SPDX-License-Identifier: MIT */
#include "frame_tap_platform.h"

/* Frame-tap callbacks can be supplied by C++ clients, and an exception that unwinds into the Rust
   frame tap is undefined. MSVC raises a C++ throw as structured exception 0xE06D7363, so this C leaf
   contains it with a filter that leaves every other fault (access violations included) alone. Report
   callbacks are skipped on failure and decisions decline. Other toolchains call the callback
   directly. */
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

static void call_log(rsf_frame_tap_log_fn fn, void* user, const char* message)
{
    CONTAIN_TRY { fn(user, message); }
    CONTAIN_CATCH {}
}

static void call_pass(rsf_frame_tap_fn fn, void* user, const rsf_frame_tap_pass* pass)
{
    CONTAIN_TRY { fn(user, pass); }
    CONTAIN_CATCH {}
}

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

static void call_constants(rsf_frame_tap_constants_fn fn, void* user, void* buffer, void* contents,
                           uint32_t bytes)
{
    CONTAIN_TRY { fn(user, buffer, contents, bytes); }
    CONTAIN_CATCH {}
}

static int32_t call_gate(rsf_frame_tap_gate_fn fn, void* user, void* context, void* texture)
{
    CONTAIN_TRY { return fn(user, context, texture); }
    CONTAIN_CATCH { return 0; }
}

static rsf_frame_tap_verdict call_verdict(rsf_frame_tap_verdict_fn fn, void* user,
                                          const rsf_frame_tap_target_draw* facts)
{
    CONTAIN_TRY { return fn(user, facts); }
    CONTAIN_CATCH { return RSF_FRAME_TAP_LEAVE; }
}

static int32_t call_override(rsf_frame_tap_constant_override_fn fn, void* user,
                             const rsf_frame_tap_target_draw* facts, uint32_t* slots, void** buffers)
{
    CONTAIN_TRY { return fn(user, facts, slots, buffers); }
    CONTAIN_CATCH { return 0; }
}

static const rsf_frame_tap_platform callbacks = {
    .log = call_log,
    .pass = call_pass,
    .report = call_report,
    .geometry = call_geometry,
    .compute = call_compute,
    .constants = call_constants,
    .gate = call_gate,
    .verdict = call_verdict,
    .override_constants = call_override,
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
