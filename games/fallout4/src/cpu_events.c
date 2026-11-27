/* SPDX-License-Identifier: MIT */
/* Native leaves of the Rust CPU event owner (cpu_events.rs). Rust owns admission, the Main poll
   source, the worker successor handoff, the GameSDK CPU facts and receipts, the PeekMessageA
   import cell and the hook lifecycle.

   This file keeps every native detour shell. A shell captures its return address and last error,
   lets Rust decide on entry, restores that error, calls the original engine function through its
   typed MinHook trampoline on this C frame, captures the native last error, lets Rust decide on
   exit and restores the native error. No Rust frame is on the stack during an engine call, so a
   native C++ unwind crosses only these shells. A shell swallows nothing: its termination handler
   cancels the entry (raw input retires it) and the exception keeps propagating. The per-call
   receipt lives on the shell's stack. */
#include "cpu_events_platform.h"
#include "native_guard.h"
#include <string.h>

typedef void (*Swap)(void*);
typedef void (*RawInput)(void*, const void*);
typedef void (*Poll)(void*, float);
typedef void (*Consume)(void*, void*);
typedef uint32_t (*WorkerJoin)(void);
typedef BOOL(WINAPI* Peek)(LPMSG, HWND, UINT, UINT, UINT);

/* Cancellation runs between its own last-error save and restore, so an unwind never changes
   the error the engine's handler reads. */
static void cancel_scope(uint32_t hook, rsf_fallout4_cpu_scope* scope)
{
    const DWORD error = GetLastError();
    rsf_fallout4_cpu_cancel(hook, scope);
    SetLastError(error);
}

static void observe_swap(void* main)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    const DWORD entry_error = GetLastError();
    rsf_fallout4_cpu_scope scope;
    memset(&scope, 0, sizeof(scope));
    rsf_fallout4_cpu_swap_enter(caller, main, &scope);
    volatile bool returned = false;
    DWORD native_error = 0;
    RSF_FOREIGN_TRY {
        SetLastError(entry_error);
        ((Swap)rsf_fallout4_cpu_original(RSF_FALLOUT4_CPU_HOOK_SWAP))(main);
        native_error = GetLastError();
        returned = true;
    }
    RSF_FOREIGN_FINALLY {
        if (!returned) cancel_scope(RSF_FALLOUT4_CPU_HOOK_SWAP, &scope);
    }
    rsf_fallout4_cpu_swap_exit(&scope);
    rsf_fallout4_cpu_swap_leave(&scope);
    SetLastError(native_error);
}

/* Raw input retires on return and on an unwind, around the native error the original left. */
static void observe_raw(uint32_t hook, uintptr_t caller, void* device, const void* raw)
{
    const DWORD error = GetLastError();
    rsf_fallout4_cpu_raw_enter(hook, caller);
    RSF_FOREIGN_TRY {
        SetLastError(error);
        ((RawInput)rsf_fallout4_cpu_original(hook))(device, raw);
    }
    RSF_FOREIGN_FINALLY {
        const DWORD native_error = GetLastError();
        rsf_fallout4_cpu_raw_exit();
        SetLastError(native_error);
    }
}

static void observe_keyboard(void* device, const void* raw)
{
    observe_raw(RSF_FALLOUT4_CPU_HOOK_KEYBOARD, RSF_FALLOUT4_CALLER(), device, raw);
}

static void observe_mouse(void* device, const void* raw)
{
    observe_raw(RSF_FALLOUT4_CPU_HOOK_MOUSE, RSF_FALLOUT4_CALLER(), device, raw);
}

static void observe_idle(void* main)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    const DWORD error = GetLastError();
    rsf_fallout4_cpu_scope scope;
    memset(&scope, 0, sizeof(scope));
    rsf_fallout4_cpu_idle_enter(caller, main, &scope);
    volatile bool returned = false;
    DWORD native_error = 0;
    RSF_FOREIGN_TRY {
        SetLastError(error);
        ((Swap)rsf_fallout4_cpu_original(RSF_FALLOUT4_CPU_HOOK_IDLE))(main);
        native_error = GetLastError();
        returned = true;
    }
    RSF_FOREIGN_FINALLY {
        if (!returned) cancel_scope(RSF_FALLOUT4_CPU_HOOK_IDLE, &scope);
    }
    rsf_fallout4_cpu_idle_exit(&scope);
    SetLastError(native_error);
}

static void observe_poll(void* manager, float delta)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    const DWORD error = GetLastError();
    rsf_fallout4_cpu_scope scope;
    memset(&scope, 0, sizeof(scope));
    rsf_fallout4_cpu_poll_enter(caller, manager, &scope);
    volatile bool returned = false;
    DWORD native_error = 0;
    LARGE_INTEGER sample_begin;
    sample_begin.QuadPart = 0;
    RSF_FOREIGN_TRY {
        SetLastError(error);
        /* RESERVE must have returned before taking the poll's entry timestamp. */
        QueryPerformanceCounter(&sample_begin);
        ((Poll)rsf_fallout4_cpu_original(RSF_FALLOUT4_CPU_HOOK_POLL))(manager, delta);
        native_error = GetLastError();
        returned = true;
    }
    RSF_FOREIGN_FINALLY {
        if (!returned) cancel_scope(RSF_FALLOUT4_CPU_HOOK_POLL, &scope);
    }
    rsf_fallout4_cpu_poll_exit(&scope, (uint64_t)sample_begin.QuadPart);
    SetLastError(native_error);
}

static void observe_consume(void* queue, void* receiver)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    const DWORD error = GetLastError();
    rsf_fallout4_cpu_scope scope;
    memset(&scope, 0, sizeof(scope));
    rsf_fallout4_cpu_consume_enter(caller, queue, receiver, &scope);
    volatile bool returned = false;
    DWORD native_error = 0;
    RSF_FOREIGN_TRY {
        SetLastError(error);
        ((Consume)rsf_fallout4_cpu_original(RSF_FALLOUT4_CPU_HOOK_CONSUME))(queue, receiver);
        native_error = GetLastError();
        returned = true;
    }
    RSF_FOREIGN_FINALLY {
        if (!returned) cancel_scope(RSF_FALLOUT4_CPU_HOOK_CONSUME, &scope);
    }
    rsf_fallout4_cpu_consume_exit(&scope);
    SetLastError(native_error);
}

static void observe_player_controls(void* receiver, void* events)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    const DWORD error = GetLastError();
    rsf_fallout4_cpu_scope scope;
    memset(&scope, 0, sizeof(scope));
    rsf_fallout4_cpu_player_controls_enter(caller, receiver, events, &scope);
    volatile bool returned = false;
    DWORD native_error = 0;
    RSF_FOREIGN_TRY {
        SetLastError(error);
        ((Consume)rsf_fallout4_cpu_original(RSF_FALLOUT4_CPU_HOOK_PLAYER_CONTROLS))(receiver, events);
        native_error = GetLastError();
        returned = true;
    }
    RSF_FOREIGN_FINALLY {
        if (!returned) cancel_scope(RSF_FALLOUT4_CPU_HOOK_PLAYER_CONTROLS, &scope);
    }
    rsf_fallout4_cpu_player_controls_exit(&scope);
    SetLastError(native_error);
}

static void observe_job_end(void* argument)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    const DWORD error = GetLastError();
    rsf_fallout4_cpu_scope scope;
    memset(&scope, 0, sizeof(scope));
    rsf_fallout4_cpu_job_end_enter(caller, &scope);
    volatile bool returned = false;
    DWORD native_error = 0;
    RSF_FOREIGN_TRY {
        SetLastError(error);
        ((Swap)rsf_fallout4_cpu_original(RSF_FALLOUT4_CPU_HOOK_JOB_END))(argument);
        native_error = GetLastError();
        returned = true;
    }
    RSF_FOREIGN_FINALLY {
        if (!returned) cancel_scope(RSF_FALLOUT4_CPU_HOOK_JOB_END, &scope);
    }
    rsf_fallout4_cpu_job_end_exit(&scope);
    SetLastError(native_error);
}

static void observe_wrapper(void* main)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    const DWORD error = GetLastError();
    rsf_fallout4_cpu_scope scope;
    memset(&scope, 0, sizeof(scope));
    rsf_fallout4_cpu_wrapper_enter(caller, main, &scope);
    volatile bool returned = false;
    DWORD native_error = 0;
    RSF_FOREIGN_TRY {
        SetLastError(error);
        ((Swap)rsf_fallout4_cpu_original(RSF_FALLOUT4_CPU_HOOK_WRAPPER))(main);
        native_error = GetLastError();
        returned = true;
    }
    RSF_FOREIGN_FINALLY {
        if (!returned) cancel_scope(RSF_FALLOUT4_CPU_HOOK_WRAPPER, &scope);
    }
    rsf_fallout4_cpu_wrapper_exit(&scope);
    SetLastError(native_error);
}

static uint32_t observe_worker_join(void)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    const DWORD error = GetLastError();
    rsf_fallout4_cpu_scope scope;
    memset(&scope, 0, sizeof(scope));
    rsf_fallout4_cpu_worker_join_enter(caller, &scope);
    volatile bool returned = false;
    DWORD native_error = 0;
    uint32_t result = 0;
    RSF_FOREIGN_TRY {
        SetLastError(error);
        result = ((WorkerJoin)rsf_fallout4_cpu_original(RSF_FALLOUT4_CPU_HOOK_WORKER_JOIN))();
        native_error = GetLastError();
        returned = true;
    }
    RSF_FOREIGN_FINALLY {
        if (!returned) cancel_scope(RSF_FALLOUT4_CPU_HOOK_WORKER_JOIN, &scope);
    }
    rsf_fallout4_cpu_worker_join_exit(&scope, result);
    SetLastError(native_error);
    return result;
}

/* Lives in the import cell. The module stays pinned after stop because a later IAT owner can
   retain this address; Rust keeps its continuation unchanged after publication. */
static BOOL WINAPI observe_peek(LPMSG message, HWND window, UINT minimum, UINT maximum, UINT flags)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    const DWORD error = GetLastError();
    const Peek next = (Peek)rsf_fallout4_cpu_peek_enter();
    volatile bool returned = false;
    DWORD native_error = 0;
    BOOL result = FALSE;
    RSF_FOREIGN_TRY {
        SetLastError(error);
        result = next(message, window, minimum, maximum, flags);
        native_error = GetLastError();
        returned = true;
    }
    RSF_FOREIGN_FINALLY {
        if (!returned) {
            const DWORD cancel_error = GetLastError();
            rsf_fallout4_cpu_peek_cancel();
            SetLastError(cancel_error);
        }
    }
    rsf_fallout4_cpu_peek_exit(caller, result, message, window, minimum, maximum, flags);
    SetLastError(native_error);
    return result;
}

/* RSF_FALLOUT4_CPU_HOOK_* order. */
void* rsf_fallout4_cpu_detour(uint32_t hook)
{
    switch (hook) {
    case RSF_FALLOUT4_CPU_HOOK_SWAP: return (void*)&observe_swap;
    case RSF_FALLOUT4_CPU_HOOK_KEYBOARD: return (void*)&observe_keyboard;
    case RSF_FALLOUT4_CPU_HOOK_MOUSE: return (void*)&observe_mouse;
    case RSF_FALLOUT4_CPU_HOOK_IDLE: return (void*)&observe_idle;
    case RSF_FALLOUT4_CPU_HOOK_POLL: return (void*)&observe_poll;
    case RSF_FALLOUT4_CPU_HOOK_CONSUME: return (void*)&observe_consume;
    case RSF_FALLOUT4_CPU_HOOK_PLAYER_CONTROLS: return (void*)&observe_player_controls;
    case RSF_FALLOUT4_CPU_HOOK_JOB_END: return (void*)&observe_job_end;
    case RSF_FALLOUT4_CPU_HOOK_WRAPPER: return (void*)&observe_wrapper;
    case RSF_FALLOUT4_CPU_HOOK_WORKER_JOIN: return (void*)&observe_worker_join;
    default: return NULL;
    }
}

void* rsf_fallout4_cpu_peek_thunk(void)
{
    return (void*)&observe_peek;
}

bool rsf_fallout4_cpu_deliver(rsf_game_cpu_event_fn callback, void* user, const rsf_game_cpu_event* event)
{
    bool returned = true;
    RSF_FOREIGN_TRY { callback(user, event); }
    RSF_FOREIGN_CATCH { returned = false; }
    return returned;
}
