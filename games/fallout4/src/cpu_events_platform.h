// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between the Fallout 4 CPU event leaves (cpu_events.c) and their Rust owner
// (cpu_events.rs). Not a DLL ABI: both sides link into ReScaleFrame.Game.Fallout4.dll.
// tools/generate-rust-bindings.py generates src/cpu_abi.rs from this header.
#include <stdbool.h>
#include <stdint.h>
#include <rescaleframe/game_api.h>

// The CPU source identity borrowed during the guarded Swap.
typedef struct rsf_fallout4_cpu_identity {
    uint64_t session;
    uint64_t source;
    uint64_t submission;
    uint64_t began_qpc;
    uint64_t frequency;
} rsf_fallout4_cpu_identity;

// MinHook detours in creation, enable, disable and removal order.
#define RSF_FALLOUT4_CPU_HOOK_SWAP 0u            /* void(void* main) */
#define RSF_FALLOUT4_CPU_HOOK_KEYBOARD 1u        /* void(void* device, const void* raw) */
#define RSF_FALLOUT4_CPU_HOOK_MOUSE 2u           /* void(void* device, const void* raw) */
#define RSF_FALLOUT4_CPU_HOOK_IDLE 3u            /* void(void* main) */
#define RSF_FALLOUT4_CPU_HOOK_POLL 4u            /* void(void* manager, float delta) */
#define RSF_FALLOUT4_CPU_HOOK_CONSUME 5u         /* void(void* queue, void* receiver) */
#define RSF_FALLOUT4_CPU_HOOK_PLAYER_CONTROLS 6u /* void(void* receiver, void* events) */
#define RSF_FALLOUT4_CPU_HOOK_JOB_END 7u         /* void(void* argument) */
#define RSF_FALLOUT4_CPU_HOOK_WRAPPER 8u         /* void(void* main) */
#define RSF_FALLOUT4_CPU_HOOK_WORKER_JOIN 9u     /* uint32_t(void) */
#define RSF_FALLOUT4_CPU_HOOK_COUNT 10u

// One detour call's receipt on its native shell's stack. The Rust entry writes it with its hook
// tag and the exit consumes it; the shell never reads it.
#define RSF_FALLOUT4_CPU_SCOPE_WORDS 64u
typedef struct rsf_fallout4_cpu_scope {
    uint64_t tag;
    uint64_t state[RSF_FALLOUT4_CPU_SCOPE_WORDS];
} rsf_fallout4_cpu_scope;

// Linked native leaves. Delivery takes the Rust owner's validated nonnull callback. The detours and
// the message thunk are in cpu_events.c.
void* rsf_fallout4_cpu_detour(uint32_t hook);
void* rsf_fallout4_cpu_peek_thunk(void);
bool rsf_fallout4_cpu_deliver(rsf_game_cpu_event_fn callback, void* user, const rsf_game_cpu_event* event);

// Rust owner entry points.
void* rsf_fallout4_cpu_original(uint32_t hook);

// Detour entry and exit decisions. caller is the shell's return address. Each shell calls entry,
// restores its entry last error, calls the original through its trampoline, captures the native
// last error, calls exit and restores that error. None of these throws or unwinds.
void rsf_fallout4_cpu_raw_enter(uint32_t hook, uintptr_t caller); /* KEYBOARD or MOUSE */
void rsf_fallout4_cpu_raw_exit(void);
void rsf_fallout4_cpu_idle_enter(uintptr_t caller, void* main, rsf_fallout4_cpu_scope* scope);
void rsf_fallout4_cpu_idle_exit(rsf_fallout4_cpu_scope* scope);
void rsf_fallout4_cpu_poll_enter(uintptr_t caller, void* manager, rsf_fallout4_cpu_scope* scope);
// sample_begin is the QPC taken after restoring the entry error, immediately before Poll.
void rsf_fallout4_cpu_poll_exit(rsf_fallout4_cpu_scope* scope, uint64_t sample_begin);
void rsf_fallout4_cpu_consume_enter(uintptr_t caller, void* queue, void* receiver,
    rsf_fallout4_cpu_scope* scope);
void rsf_fallout4_cpu_consume_exit(rsf_fallout4_cpu_scope* scope);
void rsf_fallout4_cpu_player_controls_enter(uintptr_t caller, void* receiver, void* events,
    rsf_fallout4_cpu_scope* scope);
void rsf_fallout4_cpu_player_controls_exit(rsf_fallout4_cpu_scope* scope);
// Swap exit runs after a returned native Swap and leave then closes the scope; a native unwind
// runs cancel instead.
void rsf_fallout4_cpu_swap_enter(uintptr_t caller, void* main, rsf_fallout4_cpu_scope* scope);
void rsf_fallout4_cpu_swap_exit(rsf_fallout4_cpu_scope* scope);
void rsf_fallout4_cpu_swap_leave(rsf_fallout4_cpu_scope* scope);
void rsf_fallout4_cpu_job_end_enter(uintptr_t caller, rsf_fallout4_cpu_scope* scope);
void rsf_fallout4_cpu_job_end_exit(rsf_fallout4_cpu_scope* scope);
void rsf_fallout4_cpu_wrapper_enter(uintptr_t caller, void* main, rsf_fallout4_cpu_scope* scope);
void rsf_fallout4_cpu_wrapper_exit(rsf_fallout4_cpu_scope* scope);
void rsf_fallout4_cpu_worker_join_enter(uintptr_t caller, rsf_fallout4_cpu_scope* scope);
// result is the native join's EAX.
void rsf_fallout4_cpu_worker_join_exit(rsf_fallout4_cpu_scope* scope, uint32_t result);
// Returns the immutable PeekMessageA continuation; exit sees the native result and arguments.
void* rsf_fallout4_cpu_peek_enter(void);
void rsf_fallout4_cpu_peek_exit(uintptr_t caller, int32_t result, void* message, void* window,
    uint32_t minimum, uint32_t maximum, uint32_t flags);
// Cancellation when the original unwinds before returning its result.
void rsf_fallout4_cpu_cancel(uint32_t hook, rsf_fallout4_cpu_scope* scope);
void rsf_fallout4_cpu_peek_cancel(void);

