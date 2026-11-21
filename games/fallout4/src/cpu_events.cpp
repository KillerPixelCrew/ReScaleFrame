// SPDX-License-Identifier: MIT
// Native leaves of the Rust CPU event owner (cpu_events.rs). Rust owns admission, the Main poll
// source, the worker successor handoff, the GameSDK CPU facts and receipts, the PeekMessageA
// import cell and the hook lifecycle. This file keeps the typed detour shells, which forward
// their arguments and return address, the MinHook trampolines and their typed calls, the guarded
// host callback and the resident PeekMessageA thunk.
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
// Written by MinHook at creation and cleared only after a successful removal.
std::array<void*, RSF_FALLOUT4_CPU_HOOK_COUNT> trampolines{};

void observe_swap(void* main) { rsf_fallout4_cpu_swap(RSF_FALLOUT4_CALLER(), main); }
void observe_keyboard(void* device, const void* raw) { rsf_fallout4_cpu_keyboard(RSF_FALLOUT4_CALLER(), device, raw); }
void observe_mouse(void* device, const void* raw) { rsf_fallout4_cpu_mouse(RSF_FALLOUT4_CALLER(), device, raw); }
void observe_idle(void* main) { rsf_fallout4_cpu_idle(RSF_FALLOUT4_CALLER(), main); }
void observe_poll(void* manager, float delta) { rsf_fallout4_cpu_poll(RSF_FALLOUT4_CALLER(), manager, delta); }
void observe_consume(void* queue, void* receiver) { rsf_fallout4_cpu_consume(RSF_FALLOUT4_CALLER(), queue, receiver); }
void observe_player_controls(void* receiver, void* events) {
    rsf_fallout4_cpu_player_controls(RSF_FALLOUT4_CALLER(), receiver, events);
}
void observe_job_end(void* argument) { rsf_fallout4_cpu_job_end(RSF_FALLOUT4_CALLER(), argument); }
void observe_wrapper(void* main) { rsf_fallout4_cpu_wrapper(RSF_FALLOUT4_CALLER(), main); }
uint32_t observe_worker_join() { return rsf_fallout4_cpu_worker_join(RSF_FALLOUT4_CALLER()); }
// Lives in the import cell. The module stays pinned after stop because a later IAT owner can
// retain this address; Rust keeps its continuation unchanged after publication.
BOOL WINAPI observe_peek(LPMSG message, HWND window, UINT minimum, UINT maximum, UINT flags) {
    return rsf_fallout4_cpu_peek(RSF_FALLOUT4_CALLER(), message, window, minimum, maximum, flags);
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
// Nothing here touches the thread's last error before the trampoline call.
uint32_t call_original(uint32_t hook, void* first, const void* second, float delta) noexcept {
    void* const target = hook < trampolines.size() ? trampolines[hook] : nullptr;
    if (!target) return 0;
    switch (hook) {
    case RSF_FALLOUT4_CPU_HOOK_SWAP:
    case RSF_FALLOUT4_CPU_HOOK_IDLE:
    case RSF_FALLOUT4_CPU_HOOK_JOB_END:
    case RSF_FALLOUT4_CPU_HOOK_WRAPPER:
        reinterpret_cast<Swap>(target)(first);
        return 0;
    case RSF_FALLOUT4_CPU_HOOK_KEYBOARD:
    case RSF_FALLOUT4_CPU_HOOK_MOUSE:
        reinterpret_cast<RawInput>(target)(first, second);
        return 0;
    case RSF_FALLOUT4_CPU_HOOK_POLL:
        reinterpret_cast<Poll>(target)(first, delta);
        return 0;
    case RSF_FALLOUT4_CPU_HOOK_CONSUME:
    case RSF_FALLOUT4_CPU_HOOK_PLAYER_CONTROLS:
        reinterpret_cast<Consume>(target)(first, const_cast<void*>(second));
        return 0;
    case RSF_FALLOUT4_CPU_HOOK_WORKER_JOIN:
        return reinterpret_cast<WorkerJoin>(target)();
    default:
        return 0;
    }
}
bool cpu_event(rsf_game_cpu_event_fn callback, void* user, const rsf_game_cpu_event* event) noexcept {
    if (!callback) return false;
    try { callback(user, event); return true; } catch (...) { return false; }
}
void finish_scope() noexcept { finish_render_scope(); }
void* peek_thunk() noexcept { return reinterpret_cast<void*>(&observe_peek); }

constexpr rsf_fallout4_cpu_platform platform{
    create_hook, hook_created, remove_hook, call_original, cpu_event, finish_scope, peek_thunk};
}
const rsf_fallout4_cpu_platform* cpu_event_platform() noexcept { return &platform; }
}
