// SPDX-License-Identifier: GPL-3.0-only
#include "cpu_events.h"
#include "render_hooks.h"
#include <MinHook.h>
#include <windows.h>
#include <atomic>
#include <array>
#include <cstring>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

namespace rsf::fallout4 {
namespace {
using Swap = void (*)(void*);
Swap original{};
using RawInput = void (*)(void*, const void*);
RawInput original_keyboard{}, original_mouse{};
Swap original_idle{};
using Poll = void (*)(void*, float);
Poll original_poll{};
using Consume = void (*)(void*, void*);
Consume original_consume{};
Consume original_player_controls{};
rsf_game_host_services host{};
std::atomic<bool> enabled{};
std::atomic<uint32_t> entered{};
std::atomic<uint64_t> next_source{1}, next_submission{1};
std::atomic<uint64_t> poll_generation{}, raw_revision{};
std::atomic<uint32_t> polls{}, raw_writers{};
thread_local CpuIdentity identity{};
thread_local uint32_t rendered{};
thread_local void* idle_main{};
thread_local uint32_t idle_depth{}, swap_depth{};
thread_local bool source_invalid{}, swap_admitted{}, consumer_admitted{};
struct NativeState {
    uintptr_t main{}, manager{}, queue{}, receiver{};
    std::array<unsigned char, 8> flags{};
    unsigned char menu{}, polling_enabled{};
};
struct Pending {
    uint64_t source{}, generation{}, raw{}, sample_begin{}, frequency{};
    uint32_t queue_generation{};
    NativeState state{};
    bool sampled{}, bound{};
};
thread_local Pending pending{};
uintptr_t image() { return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)); }
template<class T> bool read(uintptr_t address, T& value) {
    SIZE_T copied{};
    return ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<const void*>(address),
        &value, sizeof(value), &copied) && copied == sizeof(value);
}
bool state(NativeState& value) {
    uintptr_t vtable{};
    return idle_main && read(image() + 0x32dd130, value.main) && value.main == uintptr_t(idle_main) &&
        read(image() + 0x3e75cb8, value.manager) && value.manager &&
        read(value.manager + 0x35, value.polling_enabled) && value.polling_enabled &&
        read(image() + 0x3273ae0, value.queue) && value.queue &&
        read(image() + 0x31f0b80, value.receiver) && value.receiver &&
        read(value.receiver, vtable) && vtable == image() + 0x2571a30 &&
        read(value.main + 0x24, value.flags) && read(value.main + 0x1d0, value.menu) &&
        !value.flags[0] && !value.flags[1] && !value.flags[2] && value.flags[3] &&
        !value.flags[5] && !value.flags[6] && !value.flags[7] && !value.menu;
}
bool same_state(const NativeState& a, const NativeState& b) {
    return a.main == b.main && a.manager == b.manager && a.queue == b.queue &&
        a.receiver == b.receiver && a.flags == b.flags && a.menu == b.menu &&
        a.polling_enabled == b.polling_enabled;
}
bool bytes(uintptr_t rva, const unsigned char* expected, size_t size) {
    unsigned char actual[16]{};
    SIZE_T copied{};
    return size <= sizeof(actual) && ReadProcessMemory(GetCurrentProcess(),
        reinterpret_cast<const void*>(image() + rva), actual, size, &copied) &&
        copied == size && std::memcmp(actual, expected, size) == 0;
}
bool guards() {
    constexpr unsigned char entry[]{0x48,0x89,0x5c,0x24,0x08,0x57,0x48,0x81,0xec,0x40,0x01,0x00,0x00};
    constexpr unsigned char call[]{0xe8,0x9f,0xeb,0xff,0xff};
    constexpr unsigned char keyboard[]{0x48,0x89,0x5c,0x24,0x10,0x48,0x89,0x6c,0x24,0x18};
    constexpr unsigned char mouse[]{0x0f,0xb7,0x42,0x1c,0xa8,0x01,0x74,0x08};
    constexpr unsigned char keyboard_call[]{0xe8,0xae,0xa7,0xa3,0x00};
    constexpr unsigned char mouse_call[]{0xe8,0x60,0xab,0xa3,0x00};
    constexpr unsigned char idle[]{0x48,0x8b,0xc4,0x48,0x89,0x58,0x08,0x48,0x89,0x70,0x10};
    constexpr unsigned char poll[]{0x48,0x83,0xec,0x38,0x80,0x79,0x35,0x00};
    constexpr unsigned char consume[]{0x40,0x53,0x48,0x83,0xec,0x20,0x48,0x89,0x6c,0x24,0x30};
    constexpr unsigned char poll_call[]{0xe8,0x86,0xe2,0xa3,0x00};
    constexpr unsigned char consume_call[]{0xe8,0x02,0xf8,0xa3,0x00};
    constexpr unsigned char idle_call[]{0xe8,0x10,0x24,0x00,0x00};
    constexpr unsigned char player_controls[]{0x48,0x8b,0xc4,0x56,0x57,0x48,0x83,0xec,0x68};
    return bytes(0xc33250, entry, sizeof(entry)) && bytes(0xc346ac, call, sizeof(call)) &&
        bytes(0x1675140, keyboard, sizeof(keyboard)) && bytes(0x16754d0, mouse, sizeof(mouse)) &&
        bytes(0xc3a98d, keyboard_call, sizeof(keyboard_call)) && bytes(0xc3a96b, mouse_call, sizeof(mouse_call)) &&
        bytes(0xc339d0, idle, sizeof(idle)) && bytes(0x1672540, poll, sizeof(poll)) &&
        bytes(0x1673ae0, consume, sizeof(consume)) && bytes(0xc342b5, poll_call, sizeof(poll_call)) &&
        bytes(0xc342d9, consume_call, sizeof(consume_call)) && bytes(0xc315bb, idle_call, sizeof(idle_call)) &&
        bytes(0xddbe50, player_controls, sizeof(player_controls));
}
void received(uint32_t kind) noexcept {
    LARGE_INTEGER now{}, frequency{};
    QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    rsf_game_cpu_event event{};
    event.struct_size = sizeof(event); event.event_version = RSF_GAME_CPU_EVENT_VERSION;
    event.stage = RSF_GAME_CPU_INPUT_EVENT; event.session_id = host.session_id;
    event.timestamp_qpc = static_cast<uint64_t>(now.QuadPart);
    event.qpc_frequency = static_cast<uint64_t>(frequency.QuadPart);
    event.input_kind = kind; event.flags = RSF_GAME_CPU_INPUT_UNFRAMED;
    if (host.cpu_event) try { host.cpu_event(host.user, &event); } catch (...) {}
}
// These are native RAWINPUT handlers, not the later buffered Poll methods.
void observe_keyboard(void* device, const void* raw) {
    entered.fetch_add(1, std::memory_order_acq_rel);
    raw_writers.fetch_add(1); raw_revision.fetch_add(1);
#if defined(_MSC_VER)
    const auto caller = reinterpret_cast<uintptr_t>(_ReturnAddress());
#else
    const auto caller = reinterpret_cast<uintptr_t>(__builtin_return_address(0));
#endif
    const auto error = GetLastError();
    if (enabled.load(std::memory_order_acquire) && caller == image() + 0xc3a992)
        received(RSF_GAME_INPUT_KEYBOARD);
    SetLastError(error);
    original_keyboard(device, raw);
    raw_revision.fetch_add(1); raw_writers.fetch_sub(1);
    entered.fetch_sub(1, std::memory_order_acq_rel);
}
void observe_mouse(void* device, const void* raw) {
    entered.fetch_add(1, std::memory_order_acq_rel);
    raw_writers.fetch_add(1); raw_revision.fetch_add(1);
#if defined(_MSC_VER)
    const auto caller = reinterpret_cast<uintptr_t>(_ReturnAddress());
#else
    const auto caller = reinterpret_cast<uintptr_t>(__builtin_return_address(0));
#endif
    const auto error = GetLastError();
    if (enabled.load(std::memory_order_acquire) && caller == image() + 0xc3a970)
        received(RSF_GAME_INPUT_MOUSE);
    SetLastError(error);
    original_mouse(device, raw);
    raw_revision.fetch_add(1); raw_writers.fetch_sub(1);
    entered.fetch_sub(1, std::memory_order_acq_rel);
}
void emit(uint32_t stage) noexcept {
    LARGE_INTEGER now{};
    QueryPerformanceCounter(&now);
    rsf_game_cpu_event event{};
    event.struct_size = sizeof(event); event.event_version = RSF_GAME_CPU_EVENT_VERSION;
    event.stage = stage; event.session_id = identity.session;
    event.source_frame_id = identity.source;
    event.timestamp_qpc = static_cast<uint64_t>(now.QuadPart);
    event.qpc_frequency = identity.frequency;
    if (stage == RSF_GAME_CPU_FRAME_END) {
        event.render_expected = rendered;
        if (!rendered) event.flags = RSF_GAME_CPU_RENDER_SKIPPED;
    }
    if (host.cpu_event) try { host.cpu_event(host.user, &event); } catch (...) {}
}
uint64_t qpc() noexcept {
    LARGE_INTEGER value{}; QueryPerformanceCounter(&value);
    return static_cast<uint64_t>(value.QuadPart);
}
void source_event(uint32_t stage, uint32_t reason = 0) noexcept {
    rsf_game_cpu_source_event event{};
    event.header.struct_size = sizeof(event);
    event.header.event_version = RSF_GAME_CPU_EVENT_VERSION_SOURCE;
    event.header.stage = stage; event.header.session_id = host.session_id;
    event.header.source_frame_id = pending.source;
    event.header.timestamp_qpc = qpc(); event.header.qpc_frequency = pending.frequency;
    event.native_generation = pending.generation; event.reason = reason;
    if (stage == RSF_GAME_CPU_SOURCE_SAMPLED) {
        event.header.flags = RSF_GAME_CPU_SOURCE_PRODUCED;
        event.poll_begin_qpc = pending.sample_begin;
        // Delivered kinds are unknown until native event receivers run.
    }
    if (host.cpu_event) try { host.cpu_event(host.user, &event.header); } catch (...) {}
}
void abort_pending(uint32_t reason) noexcept {
    if (!pending.source) return;
    source_event(RSF_GAME_CPU_SOURCE_ABORT, reason);
    if (identity.source == pending.source) source_invalid = true;
    pending = {};
}
uint32_t invalid_pending() noexcept {
    if (!enabled.load(std::memory_order_acquire)) return RSF_GAME_CPU_ABORT_QUIESCE;
    if (polls.load() || raw_writers.load()) return RSF_GAME_CPU_ABORT_OVERLAP;
    if (poll_generation.load() != pending.generation) return RSF_GAME_CPU_ABORT_INTERVENING_POLL;
    NativeState current{};
    if (raw_revision.load() != pending.raw || !state(current) || !same_state(current, pending.state))
        return RSF_GAME_CPU_ABORT_STATE_CHANGED;
    return 0;
}
void close_frame() noexcept {
    if (!identity.source) return;
    finish_render_scope(); emit(RSF_GAME_CPU_FRAME_END);
    identity = {}; rendered = 0; source_invalid = false;
}
void observe_idle(void* main) {
    entered.fetch_add(1, std::memory_order_acq_rel);
#if defined(_MSC_VER)
    const auto caller = reinterpret_cast<uintptr_t>(_ReturnAddress());
#else
    const auto caller = reinterpret_cast<uintptr_t>(__builtin_return_address(0));
#endif
    const auto error = GetLastError();
    ++idle_depth;
    const bool owns = enabled.load(std::memory_order_acquire) && idle_depth == 1 &&
        caller == image() + 0xc315c0 && !idle_main;
    if (owns) idle_main = main;
    SetLastError(error);
    original_idle(main);
    const auto native_error = GetLastError();
    if (owns) {
        abort_pending(!enabled.load() ? RSF_GAME_CPU_ABORT_QUIESCE :
            pending.bound ? RSF_GAME_CPU_ABORT_STATE_CHANGED : RSF_GAME_CPU_ABORT_NOT_CONSUMED);
        close_frame(); idle_main = nullptr;
    }
    --idle_depth;
    entered.fetch_sub(1, std::memory_order_acq_rel); SetLastError(native_error);
}
void observe_poll(void* manager, float delta) {
    entered.fetch_add(1, std::memory_order_acq_rel);
#if defined(_MSC_VER)
    const auto caller = reinterpret_cast<uintptr_t>(_ReturnAddress());
#else
    const auto caller = reinterpret_cast<uintptr_t>(__builtin_return_address(0));
#endif
    const auto error = GetLastError();
    const auto previous_polls = polls.fetch_add(1);
    const auto generation = poll_generation.fetch_add(1) + 1;
    if (pending.source) abort_pending(previous_polls ? RSF_GAME_CPU_ABORT_OVERLAP : RSF_GAME_CPU_ABORT_INTERVENING_POLL);
    NativeState before{};
    const bool candidate = enabled.load(std::memory_order_acquire) && caller == image() + 0xc342ba &&
        idle_depth == 1 && !previous_polls && !identity.source && !raw_writers.load() && state(before) &&
        before.manager == uintptr_t(manager);
    if (candidate) {
        LARGE_INTEGER frequency{}; QueryPerformanceFrequency(&frequency);
        pending.source = next_source.fetch_add(1); pending.generation = generation;
        pending.frequency = static_cast<uint64_t>(frequency.QuadPart);
        pending.raw = raw_revision.load(); pending.state = before;
        source_event(RSF_GAME_CPU_SOURCE_RESERVE);
    }
    SetLastError(error);
    const auto sample_begin = qpc();
    // RESERVE must have returned before taking the poll's entry timestamp.
    original_poll(manager, delta);
    const auto native_error = GetLastError();
    if (candidate && pending.source && pending.generation == generation) {
        NativeState after{};
        if (polls.load() != 1 || poll_generation.load() != generation || raw_writers.load())
            abort_pending(RSF_GAME_CPU_ABORT_OVERLAP);
        else if (!enabled.load()) abort_pending(RSF_GAME_CPU_ABORT_QUIESCE);
        else if (raw_revision.load() != pending.raw || !state(after) ||
            !same_state(before, after) || !read(before.queue + 0x1490, pending.queue_generation))
            abort_pending(RSF_GAME_CPU_ABORT_STATE_CHANGED);
        else {
            pending.sample_begin = sample_begin; pending.sampled = true;
            source_event(RSF_GAME_CPU_SOURCE_SAMPLED);
        }
    }
    polls.fetch_sub(1); entered.fetch_sub(1, std::memory_order_acq_rel); SetLastError(native_error);
}
void observe_player_controls(void* receiver, void* events) {
    entered.fetch_add(1, std::memory_order_acq_rel);
#if defined(_MSC_VER)
    const auto caller = reinterpret_cast<uintptr_t>(_ReturnAddress());
#else
    const auto caller = reinterpret_cast<uintptr_t>(__builtin_return_address(0));
#endif
    const auto error = GetLastError();
    if (consumer_admitted && pending.source && !pending.bound && idle_depth == 1 && caller == image() + 0x1673b74) {
        uint32_t queue_generation{};
        const auto invalid = invalid_pending();
        if (invalid || !pending.sampled || identity.source || uintptr_t(receiver) != pending.state.receiver ||
            !read(pending.state.queue + 0x1490, queue_generation) || queue_generation != pending.queue_generation)
            abort_pending(invalid ? invalid : RSF_GAME_CPU_ABORT_STATE_CHANGED);
        else {
            identity = {host.session_id, pending.source, 0, qpc(), pending.frequency}; rendered = 0;
            emit(RSF_GAME_CPU_FRAME_BEGIN);
            // Recheck after the host callback before asserting native consumption.
            auto changed = invalid_pending();
            if (!changed && (!read(pending.state.queue + 0x1490, queue_generation) ||
                queue_generation != pending.queue_generation)) changed = RSF_GAME_CPU_ABORT_STATE_CHANGED;
            if (changed) { abort_pending(changed); close_frame(); }
            else {
                source_event(RSF_GAME_CPU_SOURCE_BIND); pending.bound = true;
                auto changed_after_bind = invalid_pending();
                if (!changed_after_bind && (!read(pending.state.queue + 0x1490, queue_generation) ||
                    queue_generation != pending.queue_generation)) changed_after_bind = RSF_GAME_CPU_ABORT_STATE_CHANGED;
                if (changed_after_bind) { abort_pending(changed_after_bind); close_frame(); }
            }
        }
    }
    SetLastError(error);
    original_player_controls(receiver, events);
    const auto native_error = GetLastError();
    entered.fetch_sub(1, std::memory_order_acq_rel); SetLastError(native_error);
}
void observe_consume(void* queue, void* receiver) {
    entered.fetch_add(1, std::memory_order_acq_rel);
    const auto error = GetLastError();
#if defined(_MSC_VER)
    const auto caller = reinterpret_cast<uintptr_t>(_ReturnAddress());
#else
    const auto caller = reinterpret_cast<uintptr_t>(__builtin_return_address(0));
#endif
    const auto parent = consumer_admitted;
    consumer_admitted = pending.source && !pending.bound && idle_depth == 1 &&
        caller == image() + 0xc342de && uintptr_t(queue) == pending.state.queue &&
        uintptr_t(receiver) == pending.state.receiver;
    SetLastError(error);
    original_consume(queue, receiver);
    const auto native_error = GetLastError();
    consumer_admitted = parent;
    entered.fetch_sub(1, std::memory_order_acq_rel); SetLastError(native_error);
}
void observe_swap(void* main) {
    entered.fetch_add(1, std::memory_order_acq_rel);
    const auto entry_error = GetLastError();
    ++swap_depth;
    const auto parent_admitted = swap_admitted;
    swap_admitted = false;
    struct Exit {
        bool parent;
        ~Exit() { swap_admitted = parent; --swap_depth; entered.fetch_sub(1, std::memory_order_acq_rel); }
    } exit{parent_admitted};
#if defined(_MSC_VER)
    const auto caller = reinterpret_cast<uintptr_t>(_ReturnAddress());
#else
    const auto caller = reinterpret_cast<uintptr_t>(__builtin_return_address(0));
#endif
    const bool owns = enabled.load(std::memory_order_acquire) &&
        caller == image() + 0xc346b1 && swap_depth == 1 && idle_depth <= 1;
    if (!owns) {
        abort_pending(RSF_GAME_CPU_ABORT_STATE_CHANGED);
        SetLastError(entry_error);
        original(main); return;
    }
    swap_admitted = true;
    const auto error = entry_error;
    if (pending.source) {
        const auto invalid = invalid_pending();
        if (invalid || !pending.bound || idle_main != main) {
            abort_pending(invalid ? invalid : RSF_GAME_CPU_ABORT_NOT_CONSUMED); close_frame();
        }
    }
    if (source_invalid) close_frame();
    if (!identity.source) {
        LARGE_INTEGER frequency{}; QueryPerformanceFrequency(&frequency);
        identity = {host.session_id, next_source.fetch_add(1), 0,
            qpc(), static_cast<uint64_t>(frequency.QuadPart)};
        rendered = 0; emit(RSF_GAME_CPU_FRAME_BEGIN);
    }
    SetLastError(error);
    original(main);
    const auto native_error = GetLastError();
    if (pending.source) {
        const auto invalid = invalid_pending();
        if (invalid) abort_pending(invalid);
    }
    close_frame(); pending = {};
    SetLastError(native_error);
}
struct Hook { uintptr_t rva; void* replacement; void** trampoline; };
std::array<Hook, 7> hooks() {
    return {{{0xc33250, reinterpret_cast<void*>(&observe_swap), reinterpret_cast<void**>(&original)},
        {0x1675140, reinterpret_cast<void*>(&observe_keyboard), reinterpret_cast<void**>(&original_keyboard)},
        {0x16754d0, reinterpret_cast<void*>(&observe_mouse), reinterpret_cast<void**>(&original_mouse)},
        {0xc339d0, reinterpret_cast<void*>(&observe_idle), reinterpret_cast<void**>(&original_idle)},
        {0x1672540, reinterpret_cast<void*>(&observe_poll), reinterpret_cast<void**>(&original_poll)},
        {0x1673ae0, reinterpret_cast<void*>(&observe_consume), reinterpret_cast<void**>(&original_consume)},
        {0xddbe50, reinterpret_cast<void*>(&observe_player_controls), reinterpret_cast<void**>(&original_player_controls)}}};
}
}
bool prepare_cpu_events(const rsf_game_host_services& services) {
    if (services.struct_size < sizeof(services) || services.abi_version != RSF_GAME_ABI_VERSION || !services.session_id ||
        !services.cpu_event || cpu_events_busy() || !guards()) return false;
    for (const auto& hook : hooks()) if (*hook.trampoline) return false;
    host = services;
    for (const auto& hook : hooks()) {
        if (MH_CreateHook(reinterpret_cast<void*>(image() + hook.rva), hook.replacement, hook.trampoline) == MH_OK) continue;
        stop_cpu_events(); return false;
    }
    return true;
}
bool start_cpu_events() {
    if (!guards()) return false;
    for (const auto& hook : hooks()) if (!*hook.trampoline) return false;
    enabled.store(true, std::memory_order_release);
    for (const auto& hook : hooks()) {
        if (MH_EnableHook(reinterpret_cast<void*>(image() + hook.rva)) == MH_OK) continue;
        quiesce_cpu_events();
        for (const auto& installed : hooks()) MH_DisableHook(reinterpret_cast<void*>(image() + installed.rva));
        return false;
    }
    return true;
}
void quiesce_cpu_events() { enabled.store(false, std::memory_order_release); }
bool cpu_events_busy() { return entered.load(std::memory_order_acquire) != 0; }
bool stop_cpu_events() {
    quiesce_cpu_events();
    if (cpu_events_busy()) return false;
    for (const auto& hook : hooks()) {
        if (!*hook.trampoline) continue;
        const auto address = reinterpret_cast<void*>(image() + hook.rva);
        const auto result = MH_DisableHook(address);
        if (result != MH_OK && result != MH_ERROR_DISABLED) return false;
        if (cpu_events_busy() || MH_RemoveHook(address) != MH_OK) return false;
        *hook.trampoline = nullptr;
    }
    return true;
}
CpuIdentity current_cpu_identity() noexcept {
    return swap_admitted && swap_depth == 1 && idle_depth <= 1 ? identity : CpuIdentity{};
}
bool cpu_source_is_current(const CpuIdentity& expected) noexcept {
    return swap_admitted && swap_depth == 1 && idle_depth <= 1 && !source_invalid && (!pending.source || !invalid_pending()) &&
        rendered == 1 && expected.source != 0 && expected.submission != 0 &&
        expected.session == identity.session && expected.source == identity.source &&
        expected.submission == identity.submission && expected.began_qpc == identity.began_qpc &&
        expected.frequency == identity.frequency;
}
bool begin_cpu_render() noexcept {
    if (!swap_admitted || swap_depth != 1 || idle_depth > 1 || !identity.source || source_invalid || rendered ||
        (pending.source && invalid_pending())) return false;
    identity.submission = next_submission.fetch_add(1);
    rendered = 1;
    return true;
}
}
