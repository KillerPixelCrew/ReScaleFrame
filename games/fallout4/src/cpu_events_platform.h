// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between the Fallout 4 CPU event leaves (cpu_events.cpp) and their Rust owner
// (cpu_events.rs). Not a DLL ABI: both sides link into ReScaleFrame.Game.Fallout4.dll.
// tools/generate-rust-bindings.py generates src/cpu_abi.rs from this header.
#include <stdbool.h>
#include <stdint.h>
#include <rescaleframe/game_api.h>

#ifdef __cplusplus
extern "C" {
#endif

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

// Leaves the Rust owner cannot express without C++ or MinHook. The table lives for the process.
// Original engine functions are never called from Rust: each shell calls its trampoline itself.
typedef struct rsf_fallout4_cpu_platform {
    // MH_CreateHook on target with the hook's native detour shell. Refuses an existing trampoline.
    bool (*create_hook)(uint32_t hook, void* target);
    // The hook's trampoline exists.
    bool (*hook_created)(uint32_t hook);
    // MH_RemoveHook; clears the trampoline only when removal succeeded.
    bool (*remove_hook)(uint32_t hook, void* target);

    // Guarded GameSDK CPU callback. False only when it threw.
    bool (*cpu_event)(rsf_game_cpu_event_fn callback, void* user, const rsf_game_cpu_event* event);
    // render_hooks' scope close, called before a CPU frame end clears its identity.
    void (*finish_render_scope)(void);
    // The resident PeekMessageA import thunk. It stays pinned after stop.
    void* (*peek_thunk)(void);
} rsf_fallout4_cpu_platform;

// Rust owner entry points.
// Binds the CPU leaves. platform must be complete and live for the process.
bool rsf_fallout4_cpu_publish(const rsf_fallout4_cpu_platform* platform);
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
// Swap exit runs after a returned native Swap; leave runs from the shell's destructor on return
// and on a native unwind and closes the scope.
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
// Render producer queries, all on the Swap thread (see cpu_events.h).
void rsf_fallout4_cpu_current_identity(rsf_fallout4_cpu_identity* identity);
bool rsf_fallout4_cpu_begin_render(void);
bool rsf_fallout4_cpu_source_is_current(const rsf_fallout4_cpu_identity* identity);
bool rsf_fallout4_cpu_render_source_is_current(const rsf_fallout4_cpu_identity* identity);
bool rsf_fallout4_cpu_begin_present_phase(const rsf_fallout4_cpu_identity* identity,
    uint64_t native_frame, uint64_t window);
void rsf_fallout4_cpu_end_present_phase(const rsf_fallout4_cpu_identity* identity);

#ifdef __cplusplus
}
#endif
