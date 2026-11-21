// SPDX-License-Identifier: MIT
// Native leaves of the Rust CPU event owner (cpu_events.rs). Rust owns admission, the Main poll
// source, the worker successor handoff, the GameSDK CPU facts and receipts, the PeekMessageA
// import cell and the hook lifecycle.
//
// This file keeps every native detour shell. A shell captures its return address and last error,
// lets Rust decide on entry, restores that error, calls the original engine function through its
// typed MinHook trampoline on this C++ frame, captures the native last error, lets Rust decide on
// exit and restores the native error. No Rust frame is on the stack during an engine call, so a
// native unwind crosses only these shells, as it did before the port; like the previous native
// detours they are not noexcept and swallow nothing. Swap keeps its scope destructor, which also
// runs on an unwind. The per-call receipt lives on the shell's stack.
#include "cpu_events.h"
#include "render_hooks.h"
#include <MinHook.h>
#include <windows.h>
#include <array>
#if defined(_MSC_VER)
#include <intrin.h>
#define RSF_FALLOUT4_CALLER() reinterpret_cast<uintptr_t>(_ReturnAddress())
#else
#define RSF_FALLOUT4_CALLER() reinterpret_cast<uintptr_t>(__builtin_return_address(0))
#endif

namespace rsf::fallout4 {
namespace {
using Swap = void (*)(void*);
using RawInput = void (*)(void*, const void*);
using Poll = void (*)(void*, float);
using Consume = void (*)(void*, void*);
using WorkerJoin = uint32_t (*)();
using Peek = BOOL (WINAPI*)(LPMSG, HWND, UINT, UINT, UINT);
// Written by MinHook at creation and cleared only after a successful removal.
std::array<void*, RSF_FALLOUT4_CPU_HOOK_COUNT> trampolines{};
template<class Function> Function original(uint32_t hook) noexcept {
    return reinterpret_cast<Function>(trampolines[hook]);
}

void observe_swap(void* main) {
    const auto caller = RSF_FALLOUT4_CALLER();
    const auto entry_error = GetLastError();
    rsf_fallout4_cpu_scope scope{};
    rsf_fallout4_cpu_swap_enter(caller, main, &scope);
    // The previous native Exit guard: restores admission, depth and the busy count on return and
    // on an unwind out of the native Swap.
    struct Leave {
        rsf_fallout4_cpu_scope* scope;
        ~Leave() { rsf_fallout4_cpu_swap_leave(scope); }
    } leave{&scope};
    SetLastError(entry_error);
    original<Swap>(RSF_FALLOUT4_CPU_HOOK_SWAP)(main);
    const auto native_error = GetLastError();
    rsf_fallout4_cpu_swap_exit(&scope);
    SetLastError(native_error);
}
void observe_raw(uint32_t hook, uintptr_t caller, void* device, const void* raw) {
    const auto error = GetLastError();
    rsf_fallout4_cpu_raw_enter(hook, caller);
    SetLastError(error);
    original<RawInput>(hook)(device, raw);
    const auto native_error = GetLastError();
    rsf_fallout4_cpu_raw_exit();
    SetLastError(native_error);
}
void observe_keyboard(void* device, const void* raw) {
    observe_raw(RSF_FALLOUT4_CPU_HOOK_KEYBOARD, RSF_FALLOUT4_CALLER(), device, raw);
}
void observe_mouse(void* device, const void* raw) {
    observe_raw(RSF_FALLOUT4_CPU_HOOK_MOUSE, RSF_FALLOUT4_CALLER(), device, raw);
}
void observe_idle(void* main) {
    const auto caller = RSF_FALLOUT4_CALLER();
    const auto error = GetLastError();
    rsf_fallout4_cpu_scope scope{};
    rsf_fallout4_cpu_idle_enter(caller, main, &scope);
    SetLastError(error);
    original<Swap>(RSF_FALLOUT4_CPU_HOOK_IDLE)(main);
    const auto native_error = GetLastError();
    rsf_fallout4_cpu_idle_exit(&scope);
    SetLastError(native_error);
}
void observe_poll(void* manager, float delta) {
    const auto caller = RSF_FALLOUT4_CALLER();
    const auto error = GetLastError();
    rsf_fallout4_cpu_scope scope{};
    rsf_fallout4_cpu_poll_enter(caller, manager, &scope);
    SetLastError(error);
    // RESERVE must have returned before taking the poll's entry timestamp.
    LARGE_INTEGER sample_begin{};
    QueryPerformanceCounter(&sample_begin);
    original<Poll>(RSF_FALLOUT4_CPU_HOOK_POLL)(manager, delta);
    const auto native_error = GetLastError();
    rsf_fallout4_cpu_poll_exit(&scope, static_cast<uint64_t>(sample_begin.QuadPart));
    SetLastError(native_error);
}
void observe_consume(void* queue, void* receiver) {
    const auto caller = RSF_FALLOUT4_CALLER();
    const auto error = GetLastError();
    rsf_fallout4_cpu_scope scope{};
    rsf_fallout4_cpu_consume_enter(caller, queue, receiver, &scope);
    SetLastError(error);
    original<Consume>(RSF_FALLOUT4_CPU_HOOK_CONSUME)(queue, receiver);
    const auto native_error = GetLastError();
    rsf_fallout4_cpu_consume_exit(&scope);
    SetLastError(native_error);
}
void observe_player_controls(void* receiver, void* events) {
    const auto caller = RSF_FALLOUT4_CALLER();
    const auto error = GetLastError();
    rsf_fallout4_cpu_scope scope{};
    rsf_fallout4_cpu_player_controls_enter(caller, receiver, events, &scope);
    SetLastError(error);
    original<Consume>(RSF_FALLOUT4_CPU_HOOK_PLAYER_CONTROLS)(receiver, events);
    const auto native_error = GetLastError();
    rsf_fallout4_cpu_player_controls_exit(&scope);
    SetLastError(native_error);
}
void observe_job_end(void* argument) {
    const auto caller = RSF_FALLOUT4_CALLER();
    const auto error = GetLastError();
    rsf_fallout4_cpu_scope scope{};
    rsf_fallout4_cpu_job_end_enter(caller, &scope);
    SetLastError(error);
    original<Swap>(RSF_FALLOUT4_CPU_HOOK_JOB_END)(argument);
    const auto native_error = GetLastError();
    rsf_fallout4_cpu_job_end_exit(&scope);
    SetLastError(native_error);
}
void observe_wrapper(void* main) {
    const auto caller = RSF_FALLOUT4_CALLER();
    const auto error = GetLastError();
    rsf_fallout4_cpu_scope scope{};
    rsf_fallout4_cpu_wrapper_enter(caller, main, &scope);
    SetLastError(error);
    original<Swap>(RSF_FALLOUT4_CPU_HOOK_WRAPPER)(main);
    const auto native_error = GetLastError();
    rsf_fallout4_cpu_wrapper_exit(&scope);
    SetLastError(native_error);
}
uint32_t observe_worker_join() {
    const auto caller = RSF_FALLOUT4_CALLER();
    const auto error = GetLastError();
    rsf_fallout4_cpu_scope scope{};
    rsf_fallout4_cpu_worker_join_enter(caller, &scope);
    SetLastError(error);
    const auto result = original<WorkerJoin>(RSF_FALLOUT4_CPU_HOOK_WORKER_JOIN)();
    const auto native_error = GetLastError();
    rsf_fallout4_cpu_worker_join_exit(&scope, result);
    SetLastError(native_error);
    return result;
}
// Lives in the import cell. The module stays pinned after stop because a later IAT owner can
// retain this address; Rust keeps its continuation unchanged after publication.
BOOL WINAPI observe_peek(LPMSG message, HWND window, UINT minimum, UINT maximum, UINT flags) {
    const auto caller = RSF_FALLOUT4_CALLER();
    const auto error = GetLastError();
    const auto next = reinterpret_cast<Peek>(rsf_fallout4_cpu_peek_enter());
    SetLastError(error);
    const BOOL result = next(message, window, minimum, maximum, flags);
    const auto native_error = GetLastError();
    rsf_fallout4_cpu_peek_exit(caller, result, message, window, minimum, maximum, flags);
    SetLastError(native_error);
    return result;
}

// RSF_FALLOUT4_CPU_HOOK_* order.
const std::array<void*, RSF_FALLOUT4_CPU_HOOK_COUNT> detours{
    reinterpret_cast<void*>(&observe_swap), reinterpret_cast<void*>(&observe_keyboard),
    reinterpret_cast<void*>(&observe_mouse), reinterpret_cast<void*>(&observe_idle),
    reinterpret_cast<void*>(&observe_poll), reinterpret_cast<void*>(&observe_consume),
    reinterpret_cast<void*>(&observe_player_controls), reinterpret_cast<void*>(&observe_job_end),
    reinterpret_cast<void*>(&observe_wrapper), reinterpret_cast<void*>(&observe_worker_join)};

bool create_hook(uint32_t hook, void* target) noexcept {
    return hook < trampolines.size() && !trampolines[hook] &&
        MH_CreateHook(target, detours[hook], &trampolines[hook]) == MH_OK;
}
bool hook_created(uint32_t hook) noexcept { return hook < trampolines.size() && trampolines[hook] != nullptr; }
bool remove_hook(uint32_t hook, void* target) noexcept {
    if (hook >= trampolines.size() || MH_RemoveHook(target) != MH_OK) return false;
    trampolines[hook] = nullptr;
    return true;
}
// The same guard the previous native emission used around the host callback.
bool cpu_event(rsf_game_cpu_event_fn callback, void* user, const rsf_game_cpu_event* event) noexcept {
    if (!callback) return false;
    try { callback(user, event); return true; } catch (...) { return false; }
}
void finish_scope() noexcept { finish_render_scope(); }
void* peek_thunk() noexcept { return reinterpret_cast<void*>(&observe_peek); }

constexpr rsf_fallout4_cpu_platform platform{
    create_hook, hook_created, remove_hook, cpu_event, finish_scope, peek_thunk};
}
const rsf_fallout4_cpu_platform* cpu_event_platform() noexcept { return &platform; }
}
