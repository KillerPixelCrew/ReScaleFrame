// SPDX-License-Identifier: GPL-3.0-only
#include "cpu_events.h"
#include "render_hooks.h"
#include "native_creation.h"
#include <MinHook.h>
#include <windows.h>
#include <atomic>
#include <array>
#include <cstring>
#include <cstddef>
#include <cstdio>
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
using Peek = BOOL (WINAPI*)(LPMSG, HWND, UINT, UINT, UINT);
// Never changed or cleared after publication: another IAT owner can retain our thunk.
std::atomic<Peek> original_peek{};
bool peek_prepared{}, peek_owned{}, peek_protection_pending{};
DWORD peek_protection{};
rsf_game_host_services host{};
std::atomic<bool> enabled{};
std::atomic<uint32_t> entered{};
std::atomic<uint64_t> next_source{1}, next_submission{1};
std::atomic<uint64_t> poll_generation{}, raw_revision{};
std::atomic<uint32_t> polls{}, raw_writers{};
std::array<std::atomic<uint32_t>, 4> source_receipts{};
std::atomic<uint32_t> source_receipt_budget{};
std::array<std::atomic<uint32_t>, 2> notification_receipts{};
thread_local CpuIdentity identity{};
thread_local uint32_t rendered{};
thread_local void* idle_main{};
thread_local uint32_t idle_depth{}, swap_depth{};
thread_local bool source_invalid{}, swap_admitted{}, consumer_admitted{};
struct Delivery { uintptr_t queue{}, receiver{}; };
thread_local Delivery delivery{};
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
enum class StateRead : unsigned {
    not_called, idle_main, main_read, main_match, manager_read, manager_null,
    polling_read, polling_disabled, queue_read, queue_null, receiver_read,
    receiver_null, vtable_read, vtable_match, flags_read, menu_read, main_flags, menu, eligible
};
bool state(NativeState& value, StateRead* receipt = nullptr) {
    const auto result=[&](StateRead stage) { if (receipt) *receipt=stage; return stage==StateRead::eligible; };
    uintptr_t vtable{};
    if (!idle_main) return result(StateRead::idle_main);
    if (!read(image()+0x32dd130,value.main)) return result(StateRead::main_read);
    if (value.main!=uintptr_t(idle_main)) return result(StateRead::main_match);
    if (!read(image()+0x3e75cb8,value.manager)) return result(StateRead::manager_read);
    if (!value.manager) return result(StateRead::manager_null);
    if (!read(value.manager+0x35,value.polling_enabled)) return result(StateRead::polling_read);
    if (!value.polling_enabled) return result(StateRead::polling_disabled);
    if (!read(image()+0x3273ae0,value.queue)) return result(StateRead::queue_read);
    if (!value.queue) return result(StateRead::queue_null);
    if (!read(image()+0x31f0b80,value.receiver)) return result(StateRead::receiver_read);
    if (!value.receiver) return result(StateRead::receiver_null);
    if (!read(value.receiver,vtable)) return result(StateRead::vtable_read);
    if (vtable!=image()+0x2571a30) return result(StateRead::vtable_match);
    if (!read(value.main+0x24,value.flags)) return result(StateRead::flags_read);
    if (!read(value.main+0x1d0,value.menu)) return result(StateRead::menu_read);
    if (value.flags[0] || value.flags[1] || value.flags[2] || !value.flags[3] ||
        value.flags[5] || value.flags[6] || value.flags[7]) return result(StateRead::main_flags);
    if (value.menu) return result(StateRead::menu);
    return result(StateRead::eligible);
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
    constexpr unsigned char idle_argument[]{0x48,0x8b,0x0d,0x75,0xbb,0x6a,0x02};
    constexpr unsigned char main_poll_argument[]{0x48,0x8b,0x0d,0x03,0x1a,0x24,0x03};
    constexpr unsigned char wrapper_poll[]{0xe8,0x78,0xd6,0xa3,0x00};
    constexpr unsigned char worker_poll[]{0xe8,0xed,0xae,0xa9,0x00};
    constexpr unsigned char player_controls[]{0x48,0x8b,0xc4,0x56,0x57,0x48,0x83,0xec,0x68};
    return bytes(0xc33250, entry, sizeof(entry)) && bytes(0xc346ac, call, sizeof(call)) &&
        bytes(0x1675140, keyboard, sizeof(keyboard)) && bytes(0x16754d0, mouse, sizeof(mouse)) &&
        bytes(0xc3a98d, keyboard_call, sizeof(keyboard_call)) && bytes(0xc3a96b, mouse_call, sizeof(mouse_call)) &&
        bytes(0xc339d0, idle, sizeof(idle)) && bytes(0x1672540, poll, sizeof(poll)) &&
        bytes(0x1673ae0, consume, sizeof(consume)) && bytes(0xc342b5, poll_call, sizeof(poll_call)) &&
        bytes(0xc342d9, consume_call, sizeof(consume_call)) && bytes(0xc315bb, idle_call, sizeof(idle_call)) &&
        bytes(0xc315b4, idle_argument, sizeof(idle_argument)) &&
        bytes(0xc342ae, main_poll_argument, sizeof(main_poll_argument)) &&
        bytes(0xc34ec3, wrapper_poll, sizeof(wrapper_poll)) && bytes(0xbd764e, worker_poll, sizeof(worker_poll)) &&
        bytes(0xddbe50, player_controls, sizeof(player_controls));
}
bool notification_guards() {
    struct Guard { uintptr_t rva; size_t size; unsigned char expected[16]; };
    constexpr Guard guards[]{
        {0xc338e4, 6, {0xff,0x15,0xae,0xd1,0x80,0x01}},
        {0xc33918, 6, {0xff,0x15,0x7a,0xd1,0x80,0x01}},
        {0xc33985, 6, {0xff,0x15,0x0d,0xd1,0x80,0x01}},
        {0xc339bb, 6, {0xff,0x15,0xd7,0xd0,0x80,0x01}},
        {0xc338d3, 8, {0xc7,0x44,0x24,0x20,0x01,0x00,0x00,0x00}},
        {0xc33907, 8, {0xc7,0x44,0x24,0x20,0x01,0x00,0x00,0x00}},
        {0xc33959, 8, {0xc7,0x44,0x24,0x20,0x01,0x00,0x00,0x00}},
        {0xc339a9, 8, {0xc7,0x44,0x24,0x20,0x01,0x00,0x00,0x00}},
        {0xc338ea, 4, {0x85,0xc0,0x74,0x34}},
        {0xc3391e, 4, {0x85,0xc0,0x75,0xce}},
        {0xc3398b, 4, {0x85,0xc0,0x74,0x36}},
        {0xc339c1, 4, {0x85,0xc0,0x75,0xcb}},
        {0xddbe50, 9, {0x48,0x8b,0xc4,0x56,0x57,0x48,0x83,0xec,0x68}},
        {0xddbffb, 4, {0x48,0x8b,0x5b,0x18}},
        {0xddc44c, 4, {0x48,0x63,0x45,0x10}},
        {0x1673aeb, 7, {0x48,0x8d,0x99,0x78,0x14,0x00,0x00}},
        {0x1673b00, 5, {0xe8,0xfb,0x58,0xfe,0xff}},
        {0x1673b30, 5, {0xe8,0xcb,0x58,0xfe,0xff}},
        {0x1673b64, 5, {0xe8,0x67,0x5b,0xfe,0xff}},
        {0x1673b90, 5, {0xe9,0x3b,0x5b,0xfe,0xff}},
        {0x1673b35, 7, {0x48,0x8b,0xbd,0x80,0x14,0x00,0x00}},
        {0x1673b48, 3, {0x39,0x47,0x20}},
        {0x1673b53, 9, {0x8b,0x85,0x90,0x14,0x00,0x00,0x89,0x46,0x08}},
        {0x1673b69, 11, {0x48,0x8b,0x06,0x48,0x8b,0xd7,0x48,0x8b,0xce,0xff,0x10}},
        {0x167465c, 15, {0x8b,0x86,0x90,0x14,0x00,0x00,0x89,0x47,0x20,0xff,0x86,0x90,0x14,0x00,0x00}},
        {0x167466b, 8, {0x48,0xc7,0x47,0x18,0x00,0x00,0x00,0x00}},
        {0x16778a2, 5, {0xe8,0x39,0xe7,0xff,0xff}},
        {0x16760df, 5, {0xe8,0x1c,0xdb,0xff,0xff}},
        {0x1673d1c, 4, {0x44,0x89,0x7f,0x08}},
        {0x1677a5f, 5, {0xe8,0x0c,0xc7,0xff,0xff}},
        {0x1677b02, 5, {0xe8,0x69,0xc6,0xff,0xff}},
        {0x1674205, 4, {0x83,0x78,0x10,0x04}},
        {0x16742a4, 7, {0xc7,0x47,0x08,0x02,0x00,0x00,0x00}},
        {0x16742d4, 5, {0xe8,0x37,0x03,0x00,0x00}},
        {0x1677bb7, 6, {0xff,0x25,0x53,0x93,0xdc,0x00}},
    };
    for (const auto& guard : guards) if (!bytes(guard.rva, guard.expected, guard.size)) return false;
    uint32_t original_thunk{}, name{}, first_thunk{};
    uint64_t imported_name{};
    constexpr unsigned char user32[]{'U','S','E','R','3','2','.','d','l','l',0};
    constexpr unsigned char peek[]{'P','e','e','k','M','e','s','s','a','g','e','A',0};
    return read(image() + 0x2ed1098, original_thunk) && original_thunk == 0x2ed1d60 &&
        read(image() + 0x2ed10a4, name) && name == 0x2ed3736 &&
        read(image() + 0x2ed10a8, first_thunk) && first_thunk == 0x2440a70 &&
        read(image() + 0x2ed1d88, imported_name) && imported_name == 0x2ed35fe &&
        bytes(0x2ed3736, user32, sizeof(user32)) && bytes(0x2ed3600, peek, sizeof(peek));
}
void received(uint32_t kind, uint32_t message = 0, bool window_message = false) noexcept {
    LARGE_INTEGER now{}, frequency{};
    QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    rsf_game_cpu_event event{};
    event.struct_size = sizeof(event); event.event_version = RSF_GAME_CPU_EVENT_VERSION;
    event.stage = RSF_GAME_CPU_INPUT_EVENT; event.session_id = host.session_id;
    event.timestamp_qpc = static_cast<uint64_t>(now.QuadPart);
    event.qpc_frequency = static_cast<uint64_t>(frequency.QuadPart);
    event.input_kind = kind; event.flags = RSF_GAME_CPU_INPUT_UNFRAMED;
    if (window_message) event.flags |= RSF_GAME_CPU_WINDOW_MESSAGE;
    event.message_id = message;
    if (host.cpu_event) try { host.cpu_event(host.user, &event); } catch (...) {}
    if ((window_message || kind == RSF_GAME_INPUT_CONTROLLER) && host.log &&
        notification_receipts[window_message ? 0 : 1].fetch_add(1) < 4) {
        char text[160]{};
        std::snprintf(text, sizeof(text), "Fallout4 input receipt emitted: source=0 kind=%u window_message=%u message=%u.",
            kind, window_message ? 1u : 0u, message);
        try { host.log(host.user, text); } catch (...) {}
    }
}
BOOL WINAPI observe_peek(LPMSG message, HWND window, UINT minimum, UINT maximum, UINT flags) {
    const auto error = GetLastError();
    entered.fetch_add(1, std::memory_order_acq_rel);
#if defined(_MSC_VER)
    const auto caller = reinterpret_cast<uintptr_t>(_ReturnAddress());
#else
    const auto caller = reinterpret_cast<uintptr_t>(__builtin_return_address(0));
#endif
    const auto next = original_peek.load(std::memory_order_acquire);
    SetLastError(error);
    const BOOL result = next(message, window, minimum, maximum, flags);
    const auto native_error = GetLastError();
    const auto base = image();
    if (enabled.load(std::memory_order_acquire) && result && message && !window && !minimum && !maximum &&
        flags == PM_REMOVE && (caller == base + 0xc338ea || caller == base + 0xc3391e ||
        caller == base + 0xc3398b || caller == base + 0xc339c1)) {
        UINT id{};
        if (read(reinterpret_cast<uintptr_t>(message) + offsetof(MSG, message), id)) received(0, id, true);
    }
    entered.fetch_sub(1, std::memory_order_acq_rel);
    SetLastError(native_error);
    return result;
}
bool delivery_current(void* queue, void* receiver) {
    uintptr_t current_queue{}, current_receiver{}, vtable{};
    return enabled.load(std::memory_order_acquire) &&
        read(image() + 0x3273ae0, current_queue) && current_queue && current_queue == uintptr_t(queue) &&
        read(image() + 0x31f0b80, current_receiver) && current_receiver && current_receiver == uintptr_t(receiver) &&
        read(current_receiver, vtable) && vtable == image() + 0x2571a30;
}
void received_controller(void* events) noexcept {
    auto node = reinterpret_cast<uintptr_t>(events);
    for (uint32_t count = 0; node && count < 256; ++count) {
        int32_t device{}, type{};
        uintptr_t next{};
        if (!read(node + 8, device) || !read(node + 0x10, type) || !read(node + 0x18, next) ||
            device < 0 || device > 2 || type < 0 || type > 6) return;
        if (device == 2 && (type == 0 || type == 4)) { received(RSF_GAME_INPUT_CONTROLLER); return; }
        node = next;
    }
}
bool pin(const void* address) {
    HMODULE module{};
    return GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
        reinterpret_cast<LPCWSTR>(address), &module) != 0;
}
void** peek_slot() { return reinterpret_cast<void**>(image() + 0x2440a98); }
bool finish_peek_protection() {
    if (!peek_protection_pending) return true;
    DWORD ignored{};
    if (!VirtualProtect(peek_slot(), sizeof(void*), peek_protection, &ignored)) return false;
    peek_protection_pending = false;
    return true;
}
bool exchange_peek(void* expected, void* desired, bool& exchanged) {
    exchanged = false;
    if (!finish_peek_protection()) return false;
    if (!VirtualProtect(peek_slot(), sizeof(void*), PAGE_READWRITE, &peek_protection)) return false;
    peek_protection_pending = true;
    exchanged = InterlockedCompareExchangePointer(peek_slot(), desired, expected) == expected;
    return finish_peek_protection();
}
bool prepare_peek() {
    Peek current{};
    if (peek_owned || peek_protection_pending || !notification_guards() ||
        !read(image() + 0x2440a98, current) || !current || current == &observe_peek) return false;
    const auto saved = original_peek.load(std::memory_order_acquire);
    // Refuse ambiguous restart when a later hook may already chain our resident thunk.
    if (saved && saved != current) return false;
    MEMORY_BASIC_INFORMATION region{};
    if (!VirtualQuery(reinterpret_cast<const void*>(current), &region, sizeof(region)) ||
        region.State != MEM_COMMIT || (region.Protect & (PAGE_GUARD | PAGE_NOACCESS)) ||
        !(region.Protect & (PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY))) return false;
    if (!pin(reinterpret_cast<const void*>(current)) || !pin(reinterpret_cast<const void*>(&observe_peek)) ||
        !pin(reinterpret_cast<const void*>(host.cpu_event))) return false;
    original_peek.store(current, std::memory_order_release);
    peek_prepared = true;
    return true;
}
bool install_peek() {
    // Instruction guards were checked before enabling our receiver detour.
    // The live import cell is checked atomically at publication instead.
    if (!peek_prepared) return false;
    bool exchanged{};
    const bool protection_ok = exchange_peek(reinterpret_cast<void*>(original_peek.load(std::memory_order_acquire)),
        reinterpret_cast<void*>(&observe_peek), exchanged);
    peek_owned = exchanged;
    return protection_ok && exchanged;
}
bool retire_peek() {
    if (!finish_peek_protection()) return false;
    if (peek_owned) {
        bool exchanged{};
        const bool protection_ok = exchange_peek(reinterpret_cast<void*>(&observe_peek),
            reinterpret_cast<void*>(original_peek.load(std::memory_order_acquire)), exchanged);
        // Successful CAS restores ours; failed CAS preserves the later writer.
        // In either case the immutable continuation and pinned thunk remain resident.
        if (!protection_ok) return false;
        peek_owned = false;
    }
    peek_prepared = false;
    return true;
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
    // Emission only: cpu_event has no return status. Core logs its own acknowledgement.
    const auto index = stage - RSF_GAME_CPU_SOURCE_RESERVE;
    if (host.log && index < source_receipts.size() &&
        (source_receipts[index].fetch_add(1) < 8 || event.header.source_frame_id % 128 == 0) &&
        source_receipt_budget.fetch_add(1) < 64) {
        char text[240]{};
        std::snprintf(text, sizeof(text),
            "Fallout4 source fact emitted: stage=%u source=%llu generation=%llu reason=%u poll_generation=%llu raw_revision=%llu.",
            stage, static_cast<unsigned long long>(event.header.source_frame_id),
            static_cast<unsigned long long>(event.native_generation), reason,
            static_cast<unsigned long long>(poll_generation.load()), static_cast<unsigned long long>(raw_revision.load()));
        try { host.log(host.user, text); } catch (...) {}
    }
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
    StateRead state_read=StateRead::not_called;
    const auto lifecycle=enabled.load(std::memory_order_acquire);
    const auto depth_at_poll=idle_depth;
    const auto source_at_poll=identity.source;
    const auto idle_at_poll=idle_main;
    const auto raw_at_poll=raw_writers.load();
    const bool candidate = lifecycle && caller == image() + 0xc342ba &&
        depth_at_poll == 1 && !previous_polls && !source_at_poll && !raw_at_poll && state(before,&state_read) &&
        before.manager == uintptr_t(manager);
    const auto receipt=[&] {
        if (!host.log) return;
        const auto base=image();
        const unsigned caller_class=caller==base+0xc342ba ? 0u : caller==base+0xc34ec8 ? 1u :
            caller==base+0xbd7653 ? 2u : 3u;
        if (caller_class==3) return; // Unknown native callers are forwarded, not classified.
        uint32_t flags{};
        for (unsigned i=0;i<before.flags.size();++i) if (before.flags[i]) flags|=1u<<i;
        const uint32_t predicates=(lifecycle ? 1u : 0u) | (depth_at_poll==1 ? 2u : 0u) |
            (!previous_polls ? 4u : 0u) | (!source_at_poll ? 8u : 0u) | (!raw_at_poll ? 16u : 0u) |
            (before.manager==uintptr_t(manager) ? 32u : 0u) | (candidate ? 64u : 0u) |
            (idle_at_poll ? 128u : 0u) | (before.main==uintptr_t(idle_at_poll) && before.main ? 256u : 0u);
        const uint64_t key=1u | (uint64_t(caller_class)<<1) | (uint64_t(predicates)<<3) |
            (uint64_t(state_read)<<12) | (uint64_t(flags & 0xefu)<<17);
        // Fixed process-wide fact set, not a source/event ledger. Startup repeats
        // use one slot instead of exhausting the budget before world transitions.
        static std::array<std::atomic<uint64_t>,32> combinations{};
        for (auto& slot : combinations) {
            auto old=slot.load(std::memory_order_acquire);
            if (old==key) return;
            if (old || !slot.compare_exchange_strong(old,key,std::memory_order_acq_rel)) {
                if (old==key) return;
                continue;
            }
            constexpr const char* stages[]{"not-called","idle-main-missing","main-read","main-association","manager-read","manager-null",
                "polling-read","polling-disabled","queue-read","queue-null","receiver-read","receiver-null",
                "vtable-read","vtable-association","flags-read","menu-read","main-eligibility","menu","eligible"};
            constexpr const char* callers[]{"main","wrapper","worker"};
            char text[760]{};
            std::snprintf(text,sizeof(text),
                "Fallout4 native poll candidate=%u class=%s caller=%p predicate_bits=0x%x state=%s partial=1 generation=%llu idle_depth=%u overlap=%u source=%llu raw_writers=%u idle_main=%p native_main=%p manager=%p/%p polling_enabled=%u main_flags_nonzero=0x%x menu=%u queue=%p receiver=%p.",
                candidate ? 1u : 0u,callers[caller_class],reinterpret_cast<void*>(caller),predicates,stages[unsigned(state_read)],
                static_cast<unsigned long long>(generation),depth_at_poll,previous_polls,static_cast<unsigned long long>(source_at_poll),
                raw_at_poll,idle_at_poll,reinterpret_cast<void*>(before.main),manager,reinterpret_cast<void*>(before.manager),
                unsigned(before.polling_enabled),flags,unsigned(before.menu),reinterpret_cast<void*>(before.queue),reinterpret_cast<void*>(before.receiver));
            try { host.log(host.user,text); } catch (...) {}
            return;
        }
    };
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
    receipt();
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
    // The selected native dispatcher still holds its outer queue read lock here.
    // Report delivery before source adoption; this is independent of pending polls.
    if (delivery.queue && delivery.receiver == uintptr_t(receiver) && caller == image() + 0x1673b74 &&
        delivery_current(reinterpret_cast<void*>(delivery.queue), receiver)) received_controller(events);
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
    const auto parent_delivery = delivery;
    delivery = caller == image() + 0xc342de && delivery_current(queue, receiver) ?
        Delivery{uintptr_t(queue), uintptr_t(receiver)} : Delivery{};
    consumer_admitted = pending.source && !pending.bound && idle_depth == 1 &&
        caller == image() + 0xc342de && uintptr_t(queue) == pending.state.queue &&
        uintptr_t(receiver) == pending.state.receiver;
    SetLastError(error);
    original_consume(queue, receiver);
    const auto native_error = GetLastError();
    consumer_admitted = parent;
    delivery = parent_delivery;
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
        !services.cpu_event || cpu_events_busy() || !verify_executable() || !guards()) return false;
    for (const auto& hook : hooks()) if (*hook.trampoline) return false;
    host = services;
    if (!prepare_peek()) return false;
    for (const auto& hook : hooks()) {
        if (MH_CreateHook(reinterpret_cast<void*>(image() + hook.rva), hook.replacement, hook.trampoline) == MH_OK) continue;
        stop_cpu_events(); return false;
    }
    return true;
}
bool start_cpu_events() {
    if (!guards() || !notification_guards()) return false;
    for (const auto& hook : hooks()) if (!*hook.trampoline) return false;
    enabled.store(true, std::memory_order_release);
    for (const auto& hook : hooks()) {
        if (MH_EnableHook(reinterpret_cast<void*>(image() + hook.rva)) == MH_OK) continue;
        quiesce_cpu_events();
        for (const auto& installed : hooks()) MH_DisableHook(reinterpret_cast<void*>(image() + installed.rva));
        return false;
    }
    if (install_peek()) return true;
    quiesce_cpu_events();
    retire_peek();
    for (const auto& hook : hooks()) MH_DisableHook(reinterpret_cast<void*>(image() + hook.rva));
    return false;
}
void quiesce_cpu_events() { enabled.store(false, std::memory_order_release); }
bool cpu_events_busy() { return entered.load(std::memory_order_acquire) != 0; }
bool stop_cpu_events() {
    quiesce_cpu_events();
    if (!retire_peek()) return false;
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
