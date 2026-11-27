/* SPDX-License-Identifier: MIT */
#include "frame_tap_platform.h"

/* Frame-tap callbacks can be supplied by C++ clients, and an exception that unwinds into the Rust
   frame tap is undefined. MSVC raises a C++ throw as structured exception 0xE06D7363, so this C leaf
   contains it with the shared filter, which leaves every other fault (access violations included)
   alone. A callback that throws is skipped. Other toolchains call the callback directly. */
#include <rescaleframe/foreign_guard.h>

void rsf_frame_tap_call_report(rsf_frame_tap_target_fn fn, void* user,
                               const rsf_frame_tap_target_draw* draw)
{
    RSF_FOREIGN_TRY { fn(user, draw); }
    RSF_FOREIGN_CATCH {}
}

void rsf_frame_tap_call_geometry(rsf_frame_tap_geometry_fn fn, void* user,
                                 const rsf_frame_tap_geometry* geometry)
{
    RSF_FOREIGN_TRY { fn(user, geometry); }
    RSF_FOREIGN_CATCH {}
}

void rsf_frame_tap_call_compute(rsf_frame_tap_compute_fn fn, void* user, void* context,
                                uint32_t x, uint32_t y, uint32_t z, void* arguments,
                                uint32_t offset)
{
    RSF_FOREIGN_TRY { fn(user, context, x, y, z, arguments, offset); }
    RSF_FOREIGN_CATCH {}
}

void rsf_frame_tap_call_constant_observer(rsf_frame_tap_constant_observer_fn fn, void* user,
                                          void* buffer, const void* contents, uint32_t bytes)
{
    RSF_FOREIGN_TRY { fn(user, buffer, contents, bytes); }
    RSF_FOREIGN_CATCH {}
}
