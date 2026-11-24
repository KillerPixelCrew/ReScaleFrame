// SPDX-License-Identifier: MIT
// The CInput detour and its unwind boundary; bootstrap/native.rs installs it.
#include "engine_slots.h"
#include "fg_stage_platform.h"
#include "input_hooks.h"
#include "native_memory.h"

void rsf_anomaly_bootstrap_input_frame(void* self);

typedef void (*input_frame_fn)(void*);

static void call_input_frame(void* self)
{
    ((input_frame_fn)rsf_anomaly_slot_original(RSF_ANOMALY_INPUT_SLOT))(self);
}

// CInput calls a native function that may unwind through C++. The in-flight ticket is retired on return and during
// that unwind: by the termination handler under MSVC, by the cleanup handler under GCC (built with -fexceptions).
#ifdef _MSC_VER
void rsf_anomaly_bootstrap_input_frame(void* self)
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
void rsf_anomaly_bootstrap_input_frame(void* self)
{
    const uint32_t ticket __attribute__((cleanup(exit_input_frame))) =
        rsf_anomaly_input_frame_begin(guarded_copy, rsf_anomaly_fg_stage_input_begin);
    call_input_frame(self);
}
#endif

