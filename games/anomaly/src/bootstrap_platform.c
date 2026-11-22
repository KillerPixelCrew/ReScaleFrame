// SPDX-License-Identifier: MIT
// Native stage calls and the CInput detour's unwind boundary.
#include "bootstrap_platform.h"
#include "engine_slots.h"
#include "fg_stage.h"
#include "fg_stage_platform.h"
#include "input_hooks.h"
#include "native_hook_ops.h"
#include "native_memory.h"
#include "overlay_platform.h"
#include "stage_entries.h"

// bootstrap/native.rs.
bool rsf_anomaly_bootstrap_install_input(void*, size_t, void*, rsf_anomaly_input_exchange,
    rsf_anomaly_input_read, const char**);
void rsf_anomaly_bootstrap_publish_input_observer(void);
bool rsf_anomaly_bootstrap_start_overlay(void*, const wchar_t*, rsf_game_log_fn, const rsf_anomaly_hook_ops*);
void rsf_anomaly_bootstrap_set_overlay_sink(void);

_Static_assert(sizeof(wchar_t) == 2, "UTF-16 paths");
_Static_assert(sizeof(rsf_anomaly_bootstrap_platform) == 72 &&
    offsetof(rsf_anomaly_bootstrap_platform, input_hook) == 48, "bootstrap platform layout");

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

static bool configure_sr(const wchar_t* ini, bool configure, rsf_game_log_fn log)
{
    return rsf_anomaly_sr_configure_native(ini, configure, log);
}
static void configure_motion(const wchar_t* ini, rsf_game_log_fn log)
{
    rsf_anomaly_motion_configure_native(ini, log);
}
static bool configure_fg(const wchar_t* ini, bool configure, void* runtime, size_t build, rsf_game_log_fn log)
{
    return rsf_anomaly_fg_configure_native(ini, configure, runtime, rsf_anomaly_sr_depth_rva(build), log);
}
static bool input_observation(const char** reason)
{
    return rsf_anomaly_overlay_install_default_input(rsf_anomaly_minhook_ops(), reason);
}
static bool input_hook(void* image, size_t build, const char** reason)
{
    // MinHook and the slot exchange take code addresses as untyped pointers.
    return rsf_anomaly_bootstrap_install_input(image, build, (void*)(uintptr_t)&input_frame,
        rsf_anomaly_compare_slot, guarded_copy, reason);
}
static bool start_overlay(void* runtime, const wchar_t* ini, rsf_game_log_fn log)
{
    return rsf_anomaly_bootstrap_start_overlay(runtime, ini, log, rsf_anomaly_minhook_ops());
}

static const rsf_anomaly_bootstrap_platform leaves = {
    .sr = configure_sr,
    .motion = configure_motion,
    .fg = configure_fg,
    .fg_installed = rsf_anomaly_fg_installed,
    .input_observation = input_observation,
    .publish_input_observer = rsf_anomaly_bootstrap_publish_input_observer,
    .input_hook = input_hook,
    .overlay = start_overlay,
    .overlay_sink = rsf_anomaly_bootstrap_set_overlay_sink,
};

const rsf_anomaly_bootstrap_platform* rsf_anomaly_bootstrap_native(void)
{
    return &leaves;
}
