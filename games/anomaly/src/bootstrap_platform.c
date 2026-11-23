// SPDX-License-Identifier: MIT
// The CInput detour and its unwind boundary. bootstrap/native.rs composes the cold-start stage calls.
#include "engine_slots.h"
#include "fg_stage_platform.h"
#include "input_hooks.h"
#include "native_memory.h"

// bootstrap/native.rs.
bool rsf_anomaly_bootstrap_install_input(void*, size_t, void*, rsf_anomaly_input_exchange,
    rsf_anomaly_input_read, const char**);

typedef void (*input_frame_fn)(void*);

static void call_input_frame(void* self)
{
    ((input_frame_fn)rsf_anomaly_slot_original(RSF_ANOMALY_INPUT_SLOT))(self);
}

// CInput calls a native function that may unwind through C++. The in-flight ticket is retired on return and during
// that unwind: by the termination handler under MSVC, by the cleanup handler under GCC (built with -fexceptions).
#ifdef _MSC_VER
static void input_frame(void* self)
{
    const uint32_t ticket = rsf_anomaly_input_frame_begin(guarded_copy, rsf_anomaly_fg_stage_input_begin);
    RSF_FOREIGN_TRY {
        call_input_frame(self);
    } RSF_FOREIGN_FINALLY {
        rsf_anomaly_input_frame_exit(ticket);
    }
}
#else
static void exit_input_frame(const uint32_t* ticket) { rsf_anomaly_input_frame_exit(*ticket); }
static void input_frame(void* self)
{
    const uint32_t ticket __attribute__((cleanup(exit_input_frame))) =
        rsf_anomaly_input_frame_begin(guarded_copy, rsf_anomaly_fg_stage_input_begin);
    call_input_frame(self);
}
#endif

bool rsf_anomaly_bootstrap_input_hook(void* image, size_t build, const char** reason)
{
    // MinHook and the slot exchange take code addresses as untyped pointers.
    return rsf_anomaly_bootstrap_install_input(image, build, (void*)(uintptr_t)&input_frame,
        rsf_anomaly_compare_slot, guarded_copy, reason);
}