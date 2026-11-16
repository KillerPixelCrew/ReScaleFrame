// SPDX-License-Identifier: GPL-3.0-only
#include "cpu_events.h"
#include "render_hooks.h"
#include "native_creation.h"
#include "worker_handoff.h"
#include <MinHook.h>
#include <windows.h>
#include <atomic>
#include <array>
#include <cstring>
#include <cstddef>
#include <cstdio>
#include <mutex>
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
Swap original_job_end{}, original_wrapper{};
using WorkerJoin = uint32_t (*)();
WorkerJoin original_worker_join{};
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
    uint64_t predecessor{};
};
thread_local Pending pending{};
thread_local uint64_t idle_serial{}, worker_source{};
thread_local bool job_selected{}, wrapper_selected{}, future_claimed{};
thread_local uint32_t job_depth{}, wrapper_depth{};
std::atomic<uint32_t> jobs{};
using detail::WorkerCycle;
using detail::WorkerTicket;
struct RenderCycle {
    WorkerCycle key{};
    CpuIdentity cpu{};
    Pending binding{};
    NativeState state{};
    uint32_t tid{};
    bool constructed{}, sealed{}, present{}, swapped{}, joining{}, closed{}, invalid{}, mapped{}, abort_pending{}, aborting{};
};
struct Future {
    WorkerTicket facts{};
    NativeState state{};
    bool busy{}, retiring{}, abort_sent{}, reserve_announced{};
};
std::mutex worker_guard;
RenderCycle current_render;
Future future;
std::array<std::atomic<uint64_t>,32> gate_receipts{};
std::atomic<uint32_t> downstream_receipts{};
RenderCycle render_copy() { std::lock_guard lock(worker_guard); return current_render; }
Future future_copy() { std::lock_guard lock(worker_guard); return future; }

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
bool state(NativeState& value, StateRead* receipt = nullptr, uintptr_t expected_main = 0) {
    const auto result=[&](StateRead stage) { if (receipt) *receipt=stage; return stage==StateRead::eligible; };
    uintptr_t vtable{};
    const auto main=expected_main ? expected_main : uintptr_t(idle_main);
    if (!main) return result(StateRead::idle_main);
    if (!read(image()+0x32dd130,value.main)) return result(StateRead::main_read);
    if (value.main!=main) return result(StateRead::main_match);
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
enum class CyclePhase : unsigned {
    other, bound_current, reserve_entry, reserve_return, sampled, consumer_begin,
    frame_return, bind_return, consumer_return, job_complete, swap_claim,
    join_entry, join_return, world_adopt, present
};
bool native_cycle(WorkerCycle& proof, NativeState& native, bool joining, CyclePhase phase=CyclePhase::other) {
    uintptr_t worker{},vtable{},destructor{},entry{},renderer{},window{};
    unsigned char running{},exiting{}; uint32_t mode{};
    const auto expected=proof;
    StateRead state_stage=StateRead::not_called;
    uint32_t reached{},passed{},first{};
    const auto test=[&](unsigned index,bool value) {
        reached|=uint32_t(1)<<index;
        if (value) passed|=uint32_t(1)<<index;
        else first=index;
        return value;
    };
    // Each term retains the original evaluation/read order and short circuit.
    if (!test(0,state(native,&state_stage,proof.main)) || !test(1,read(image()+0x32a12f0,worker)) || !test(2,worker==image()+0x32a1320) ||
        !test(3,read(worker,vtable)) || !test(4,vtable==image()+0x255e3b8) ||
        !test(5,read(vtable,destructor)) || !test(6,destructor==image()+0xbd72e0) || !test(7,read(vtable+8,entry)) || !test(8,entry==image()+0xbd7430) ||
        !test(9,read(worker+0x58,proof.start_event)) || !test(10,bool(proof.start_event)) ||
        !test(11,read(worker+0x60,proof.complete_event)) || !test(12,bool(proof.complete_event)) || !test(13,read(worker+0x68,mode)) || !test(14,!(mode!=1 && !(joining && mode==0))) ||
        !test(15,read(worker+0x6e,running)) || !test(16,bool(running)) || !test(17,read(worker+0x6c,exiting)) || !test(18,!exiting) ||
        !test(19,read(native.main+0x30,window)) || !test(20,bool(window)) || !test(21,read(image()+0x38e13a0,renderer)) || !test(22,renderer==image()+0x3a25d10) ||
        !test(23,read(renderer+0x58,proof.window)) || !test(24,proof.window==window) || !test(25,read(image()+0x3d87238,proof.frame))) {
        // No new native reads. A failed read can leave an old/partial value;
        // reached/passed and known bits, never its scalar alone, establish it.
        static std::array<std::atomic<uint64_t>,15> stages{};
        static std::atomic<uint32_t> budget{};
        const auto stage=first ? first : 26+unsigned(state_stage);
        const auto bit=uint64_t(1)<<stage;
        const auto index=unsigned(phase);
        if (host.log && index<stages.size() && !(stages[index].fetch_or(bit,std::memory_order_relaxed)&bit) &&
            budget.fetch_add(1,std::memory_order_relaxed)<32) {
            const auto error=GetLastError();
            constexpr const char* phases[]{"other","bound-current","reserve-entry","reserve-return","sampled","consumer-begin",
                "frame-return","bind-return","consumer-return","job-complete","swap-claim","join-entry","join-return","world-adopt","present"};
            const auto known=[&](unsigned read_bit) { return unsigned((passed&(uint32_t(1)<<read_bit))!=0); };
            char text[1700]{};
            std::snprintf(text,sizeof(text),
                "Fallout4 native-cycle refusal phase=%s first=%u state_stage=%u reached=0x%x passed=0x%x joining=%u source=%llu thread_source=%llu worker_source=%llu expected_native=%u frame_known=%u native=%u depths=%u/%u/%u/%u state_known=0x%x main=%p expected_main=%p manager=%p queue=%p receiver=%p menu=%u polling=%u flags=%u,%u,%u,%u,%u,%u,%u,%u worker_known=%u worker=%p vtable_known=%u vtable=%p destructor_known=%u destructor=%p run_known=%u run=%p start_known=%u start=%p complete_known=%u complete=%p mode_known=%u mode=%u running_known=%u running=%u exit_known=%u exit=%u main_window_known=%u main_window=%p renderer_known=%u renderer=%p render_window_known=%u render_window=%p",
                phases[index],first,unsigned(state_stage),reached,passed,unsigned(joining),
                static_cast<unsigned long long>(expected.source),static_cast<unsigned long long>(identity.source),static_cast<unsigned long long>(worker_source),
                expected.frame,known(25),proof.frame,idle_depth,swap_depth,job_depth,wrapper_depth,
                (unsigned(state_stage)>unsigned(StateRead::main_read)?1u:0u) |
                (unsigned(state_stage)>unsigned(StateRead::manager_read)?2u:0u) |
                (unsigned(state_stage)>unsigned(StateRead::polling_read)?4u:0u) |
                (unsigned(state_stage)>unsigned(StateRead::queue_read)?8u:0u) |
                (unsigned(state_stage)>unsigned(StateRead::receiver_read)?16u:0u) |
                (unsigned(state_stage)>unsigned(StateRead::flags_read)?32u:0u) |
                (unsigned(state_stage)>unsigned(StateRead::menu_read)?64u:0u),
                reinterpret_cast<void*>(native.main),reinterpret_cast<void*>(expected.main),reinterpret_cast<void*>(native.manager),
                reinterpret_cast<void*>(native.queue),reinterpret_cast<void*>(native.receiver),unsigned(native.menu),unsigned(native.polling_enabled),
                unsigned(native.flags[0]),unsigned(native.flags[1]),unsigned(native.flags[2]),unsigned(native.flags[3]),
                unsigned(native.flags[4]),unsigned(native.flags[5]),unsigned(native.flags[6]),unsigned(native.flags[7]),
                known(1),reinterpret_cast<void*>(worker),known(3),reinterpret_cast<void*>(vtable),known(5),reinterpret_cast<void*>(destructor),known(7),reinterpret_cast<void*>(entry),
                known(9),reinterpret_cast<void*>(proof.start_event),known(11),reinterpret_cast<void*>(proof.complete_event),known(13),mode,
                known(15),unsigned(running),known(17),unsigned(exiting),known(19),reinterpret_cast<void*>(window),known(21),reinterpret_cast<void*>(renderer),known(23),reinterpret_cast<void*>(proof.window));
            try { host.log(host.user,text); } catch (...) {}
            SetLastError(error);
        }
        return false;
    }
    proof.main=native.main; proof.worker=worker; proof.manager=native.manager; proof.queue=native.queue; proof.receiver=native.receiver;
    return true;
}

Pending ticket_pending(const Future& value) {
    Pending p{}; const auto& f=value.facts;
    p.source=f.cpu.source; p.generation=f.generation; p.raw=f.raw; p.sample_begin=f.poll_begin; p.frequency=f.cpu.frequency;
    p.queue_generation=f.cursor; p.state=value.state; p.predecessor=f.predecessor.source;
    p.sampled=f.phase>=WorkerTicket::Phase::sampled; p.bound=f.phase>=WorkerTicket::Phase::bound;
    return p;
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
    constexpr unsigned char job[]{0x48,0x89,0x5c,0x24,0x08,0x57,0x48,0x83,0xec,0x30};
    constexpr unsigned char wrapper[]{0x48,0x83,0xec,0x28,0xf3,0x0f,0x10,0x0d};
    constexpr unsigned char join[]{0x48,0x8b,0x0d,0x29,0xc1,0x6c,0x02,0x48,0x85,0xc9,0x74,0x20};
    constexpr unsigned char callback[]{0x48,0x8b,0x47,0x10,0x48,0x85,0xc0,0x74,0x04,0x33,0xc9,0xff,0x10};
    constexpr unsigned char wrapper_call[]{0xe8,0x6b,0xe5,0x05,0x00};
    constexpr unsigned char worker_consume[]{0xe8,0xf4,0xeb,0xa3,0x00};
    constexpr unsigned char join_call[]{0xe8,0x00,0x0b,0xfa,0xff};
    constexpr unsigned char join_branch[]{0x83,0x79,0x68,0x01,0x75,0x1a,0xc7,0x41,0x68,0x00,0x00,0x00,0x00};
    constexpr unsigned char join_arguments[]{0x45,0x33,0xc0,0x48,0x8b,0x49,0x60,0xba,0xff,0xff,0xff,0xff};
    constexpr unsigned char join_wait[]{0x48,0xff,0x25,0x2c,0xb1,0x86,0x01};
    uintptr_t descriptor{},name{},worker_col{},worker_destructor{},worker_entry{};
    return read(image()+0x2f30fc0,descriptor) && descriptor==image()+0xbd6850 &&
        read(image()+0x2f30fc8,name) && name==image()+0x255dd10 &&
        read(image()+0x255e3b0,worker_col) && worker_col==image()+0x29c88d8 &&
        read(image()+0x255e3b8,worker_destructor) && worker_destructor==image()+0xbd72e0 &&
        read(image()+0x255e3c0,worker_entry) && worker_entry==image()+0xbd7430 && bytes(0xbd6850,job,sizeof(job)) && bytes(0xc34eb0,wrapper,sizeof(wrapper)) && bytes(0xbd51c0,join,sizeof(join)) &&
        bytes(0xbd76d2,callback,sizeof(callback)) && bytes(0xbd6940,wrapper_call,sizeof(wrapper_call)) &&
        bytes(0xc34ee7,worker_consume,sizeof(worker_consume)) && bytes(0xc346bb,join_call,sizeof(join_call)) &&
        bytes(0xbd51cc,join_branch,sizeof(join_branch)) && bytes(0xbd51d9,join_arguments,sizeof(join_arguments)) &&
        bytes(0xbd51e5,join_wait,sizeof(join_wait)) && bytes(0xc33250, entry, sizeof(entry)) && bytes(0xc346ac, call, sizeof(call)) &&
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
bool emit_identity(const CpuIdentity& source, uint32_t stage, uint32_t expected_render) noexcept {
    LARGE_INTEGER now{};
    QueryPerformanceCounter(&now);
    rsf_game_cpu_event event{};
    event.struct_size = sizeof(event); event.event_version = RSF_GAME_CPU_EVENT_VERSION;
    event.stage = stage; event.session_id = source.session;
    event.source_frame_id = source.source;
    event.timestamp_qpc = static_cast<uint64_t>(now.QuadPart);
    event.qpc_frequency = source.frequency;
    if (stage == RSF_GAME_CPU_FRAME_END) {
        event.render_expected = expected_render;
        if (!expected_render) event.flags = RSF_GAME_CPU_RENDER_SKIPPED;
    }
    if (!host.cpu_event) return false;
    try { host.cpu_event(host.user,&event); return true; } catch (...) { return false; }
}
void emit(uint32_t stage) noexcept { emit_identity(identity,stage,rendered); }
uint64_t qpc() noexcept {
    LARGE_INTEGER value{}; QueryPerformanceCounter(&value);
    return static_cast<uint64_t>(value.QuadPart);
}
bool source_fact(const Pending& fact, uint32_t stage, uint32_t reason = 0) noexcept {
    rsf_game_cpu_source_event event{};
    event.header.struct_size = sizeof(event);
    event.header.event_version = RSF_GAME_CPU_EVENT_VERSION_SOURCE;
    event.header.stage = stage; event.header.session_id = host.session_id;
    event.header.source_frame_id = fact.source;
    event.header.timestamp_qpc = qpc(); event.header.qpc_frequency = fact.frequency;
    event.native_generation = fact.generation; event.reason = reason;
    if (stage==RSF_GAME_CPU_SOURCE_RESERVE && fact.predecessor) {
        event.header.flags=RSF_GAME_CPU_SOURCE_OVERLAPS_PREDECESSOR;
        event.predecessor_source_id=fact.predecessor;
    }
    if (stage == RSF_GAME_CPU_SOURCE_SAMPLED) {
        event.header.flags = RSF_GAME_CPU_SOURCE_PRODUCED;
        event.poll_begin_qpc = fact.sample_begin;
        // Delivered kinds are unknown until native event receivers run.
    }
    bool delivered=false;
    if (host.cpu_event) try { host.cpu_event(host.user,&event.header); delivered=true; } catch (...) {}
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
    return delivered;
}
void source_event(uint32_t stage, uint32_t reason = 0) noexcept { source_fact(pending,stage,reason); }
bool drain_current_abort() noexcept {
    Pending p{};
    {
        std::lock_guard lock(worker_guard);
        if (!current_render.abort_pending) return true;
        if (current_render.aborting) return false;
        current_render.aborting=true; p=current_render.binding;
    }
    const bool sent=source_fact(p,RSF_GAME_CPU_SOURCE_ABORT,RSF_GAME_CPU_ABORT_STATE_CHANGED);
    {
        std::lock_guard lock(worker_guard);
        if (current_render.binding.source==p.source) {
            current_render.aborting=false;
            if (sent) current_render.abort_pending=false;
        }
    }
    return sent;
}
bool drain_future() noexcept {
    Future copy{};
    {
        std::lock_guard lock(worker_guard);
        if (!future.facts.active()) return true;
        if (!future.facts.refusal || future.busy || future.retiring) return false;
        future.retiring=true; copy=future;
    }
    const bool aborted=!copy.reserve_announced || copy.abort_sent || source_fact(ticket_pending(copy),RSF_GAME_CPU_SOURCE_ABORT,copy.facts.refusal);
    const bool ended=aborted && (!copy.facts.frame_begun || emit_identity(copy.facts.cpu,RSF_GAME_CPU_FRAME_END,0));
    {
        std::lock_guard lock(worker_guard);
        if (future.facts.cpu.source==copy.facts.cpu.source) {
            if (ended) future={}; else { future.abort_sent=aborted; future.retiring=false; }
        }
    }
    return ended;
}
void invalidate_worker(uint32_t reason, bool invalidate_current, const WorkerCycle* poll_owner = nullptr) noexcept {
    {
        std::lock_guard lock(worker_guard);
        future.facts.refuse(reason);
        const bool poll_only=poll_owner && *poll_owner==current_render.key && enabled.load() &&
            !raw_writers.load() && polls.load()==1;
        if (invalidate_current && current_render.constructed && !current_render.closed && !current_render.invalid &&
            detail::invalidates_render_source(current_render.binding.source,poll_only)) {
            current_render.invalid=true;
            current_render.abort_pending=current_render.binding.source!=0;
        }
    }
    (void)drain_current_abort(); (void)drain_future();
}
bool bound_current_valid(const RenderCycle& c, bool strict_raw) noexcept {
    NativeState native{},cycle_state{}; uint32_t cursor{},queued{};
    const bool state_valid=state(native,nullptr,c.key.main) && same_state(native,c.state);
    const bool cursor_read=state_valid && read(native.receiver+8,cursor);
    auto proof=c.key;
    const bool cycle_valid=native_cycle(proof,cycle_state,c.joining,CyclePhase::bound_current) && proof==c.key && same_state(cycle_state,c.state);
    const bool queue_read=cycle_valid && read(cycle_state.queue+0x1490,queued);
    std::lock_guard lock(worker_guard);
    if (!enabled.load() || current_render.key!=c.key || current_render.invalid ||
        current_render.binding.source!=c.binding.source || !state_valid) return false;
    if (current_render.sealed && current_render.mapped) return true;
    const auto gen=poll_generation.load(), revision=raw_revision.load();
    if (!polls.load() && gen==c.binding.generation && cursor_read && cursor==c.binding.queue_generation &&
        (!strict_raw || (!raw_writers.load() && revision==c.binding.raw))) return true;
    return current_render.constructed && !current_render.closed && current_render.mapped &&
        future.busy && !future.retiring && polls.load()==1 && jobs.load()==1 && !raw_writers.load() &&
        cursor_read && queue_read && same_state(cycle_state,future.state) &&
        future.facts.protects_bound(proof,c.binding.source,c.binding.generation,gen,revision,cursor,queued,cycle_valid);
}
bool mapped_current(uint64_t source) {
    const auto c=render_copy();
    return c.cpu.source==source && !c.invalid && c.mapped &&
        ((c.sealed) || (c.binding.source && bound_current_valid(c,true)));
}
void abort_pending(uint32_t reason) noexcept {
    if (!pending.source) return;
    source_event(RSF_GAME_CPU_SOURCE_ABORT, reason);
    if (identity.source == pending.source) source_invalid = true;
    pending = {};
}
uint32_t invalid_pending() noexcept {
    if (pending.bound && mapped_current(pending.source)) return 0;
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
    finish_render_scope(); (void)drain_current_abort(); emit(RSF_GAME_CPU_FRAME_END);
    { std::lock_guard lock(worker_guard); if (current_render.cpu.source==identity.source) current_render.closed=true; }
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
    if (owns) {
        idle_main = main; ++idle_serial;
        const auto f=future_copy();
        if (f.facts.active() && (f.facts.predecessor.main!=uintptr_t(main) ||
            f.facts.predecessor.idle+1<idle_serial)) invalidate_worker(RSF_GAME_CPU_ABORT_NOT_CONSUMED,false);
    }
    SetLastError(error);
    original_idle(main);
    const auto native_error = GetLastError();
    if (owns) {
        abort_pending(!enabled.load() ? RSF_GAME_CPU_ABORT_QUIESCE :
            pending.bound ? RSF_GAME_CPU_ABORT_STATE_CHANGED : RSF_GAME_CPU_ABORT_NOT_CONSUMED);
        close_frame();
        const auto f=future_copy();
        if (f.facts.active() && f.facts.predecessor.idle<idle_serial)
            invalidate_worker(RSF_GAME_CPU_ABORT_NOT_CONSUMED,false);
        idle_main = nullptr;
    }
    --idle_depth;
    entered.fetch_sub(1, std::memory_order_acq_rel); SetLastError(native_error);
}
// First distinct scalar conjunctions, shared hard budget32. No event or core
// acknowledgement is inferred; all logging runs outside worker_guard.
void gate_receipt(uint32_t channel, uint32_t bits, uint32_t expected, const RenderCycle& c,
    const WorkerCycle& proof, const CpuIdentity& requested) noexcept {
    if (bits==expected || !host.log) return;
    const auto key=(uint64_t(channel)<<32)|bits;
    for (auto& slot : gate_receipts) {
        auto previous=slot.load();
        if (previous==key) return;
        if (previous || !slot.compare_exchange_strong(previous,key)) {
            if (previous==key) return;
            continue;
        }
        unsigned first=0;
        while (first<32 && (bits & (uint32_t(1)<<first))) ++first;
        char text[768]{};
        std::snprintf(text,sizeof(text),
            "Fallout4 CPU native gate=%u first_missing=%u bits=0x%x expected=0x%x source=%llu requested=%llu identity=%llu submission=%llu/%llu native=%u/%u window=%p/%p idle=%llu depths=%u/%u/%u/%u selected=%u/%u rendered=%u claimed=%u invalid=%u pending=%llu tid=%u/%u cycle_flags=%u%u%u%u%u jobs=%u polls=%u raw_writers=%u.",
            channel,first,bits,expected,static_cast<unsigned long long>(c.cpu.source),
            static_cast<unsigned long long>(requested.source),static_cast<unsigned long long>(identity.source),
            static_cast<unsigned long long>(requested.submission),static_cast<unsigned long long>(c.cpu.submission),
            c.key.frame,proof.frame,reinterpret_cast<void*>(c.key.window),reinterpret_cast<void*>(proof.window),
            static_cast<unsigned long long>(c.key.idle),idle_depth,swap_depth,job_depth,wrapper_depth,
            unsigned(job_selected),unsigned(wrapper_selected),rendered,unsigned(future_claimed),unsigned(source_invalid),
            static_cast<unsigned long long>(pending.source),c.tid,GetCurrentThreadId(),
            unsigned(c.constructed),unsigned(c.sealed),unsigned(c.present),unsigned(c.closed),unsigned(c.invalid),
            jobs.load(),polls.load(),raw_writers.load());
        try { host.log(host.user,text); } catch (...) {}
        return;
    }
}
template<size_t N> uint32_t predicate_bits(const std::array<bool,N>& terms) {
    static_assert(N<=32);
    uint32_t bits{};
    for (size_t i=0;i<N;++i) if (terms[i]) bits|=uint32_t(1)<<i;
    return bits;
}
bool reserve_worker_poll(void* manager, uintptr_t caller, uint64_t& generation, uint32_t& previous_polls) {
    const auto c=render_copy(); auto proof=c.key; NativeState native{};
    const auto tid=GetCurrentThreadId();
    const bool worker_call=caller==image()+0xc34ec8;
    const bool native_valid_before=worker_call && native_cycle(proof,native,false,CyclePhase::reserve_entry);
    uint32_t queued{},consumed{};
    const bool binding_unchanged=!c.binding.source || (c.binding.bound && c.binding.source==c.cpu.source &&
        same_state(c.binding.state,native) && raw_revision.load()==c.binding.raw &&
        read(native.queue+0x1490,queued) && queued==c.binding.queue_generation &&
        read(native.receiver+8,consumed) && consumed==queued);
    LARGE_INTEGER frequency{};
    if (native_valid_before) QueryPerformanceFrequency(&frequency);
    Future copy{}; uint32_t bits{}; bool registered=false;
    {
        std::lock_guard lock(worker_guard);
        // Publish the one qualified no-native-poll-yet intention with its attempt
        // counters. Main cannot observe an advanced counter without this ticket.
        previous_polls=polls.fetch_add(1);
        generation=poll_generation.fetch_add(1)+1;
        const bool source_exists=current_render.cpu.source && current_render.cpu.submission &&
            current_render.cpu.session==host.session_id && current_render.key.source==current_render.cpu.source;
        const bool binding_ready=binding_unchanged && (!c.binding.source ||
            (generation>c.binding.generation && generation-c.binding.generation==1 && raw_revision.load()==c.binding.raw));
        bits=predicate_bits(std::array<bool,18>{enabled.load(),job_selected,wrapper_selected,
            job_depth==1,wrapper_depth==1,jobs.load()==1,!previous_polls,!raw_writers.load(),
            current_render.constructed,source_exists,binding_ready,!current_render.closed,!current_render.invalid,
            c.tid!=tid,native_valid_before,proof==current_render.key,same_state(native,current_render.state),native.manager==uintptr_t(manager)});
        if (worker_call && !pending.source && bits==0x3ffffu && !future.facts.active()) {
            const CpuIdentity next{host.session_id,next_source.fetch_add(1),0,0,static_cast<uint64_t>(frequency.QuadPart)};
            if (future.facts.reserve(c.key,next,generation,true) && future.facts.begin_callback(
                c.binding.source ? c.binding.generation : 0,c.binding.raw,c.binding.queue_generation)) {
                future.state=native; future.busy=true; current_render.mapped=true;
                copy=future; worker_source=next.source; registered=true;
            } else {
                // This local candidate has not announced RESERVE or acquired any
                // host obligation. Never clear an older or published ticket here.
                future={};
            }
        }
    }
    if (!registered) {
        if (worker_call) gate_receipt(1,bits,0x3ffffu,c,proof,{});
        return false;
    }
    bool announce=false;
    {
        std::lock_guard lock(worker_guard);
        if (future.facts.cpu.source==copy.facts.cpu.source && future.facts.valid() && enabled.load() &&
            current_render.key==copy.facts.predecessor && !current_render.invalid) {
            future.reserve_announced=true; announce=true;
        } else future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
    }
    const bool sent=announce && source_fact(ticket_pending(copy),RSF_GAME_CPU_SOURCE_RESERVE);
    // The raw snapshot belongs immediately after the synchronous callback and
    // before original Poll. Callback return is not a core acceptance receipt.
    const auto revision=raw_revision.load();
    RenderCycle after{};
    {
        std::lock_guard lock(worker_guard);
        if (future.facts.cpu.source==copy.facts.cpu.source) future.facts.end_callback();
        after=current_render;
    }
    proof=copy.facts.predecessor;
    const bool native_valid=sent && enabled.load() && native_cycle(proof,native,after.joining,CyclePhase::reserve_return) &&
        proof==copy.facts.predecessor && same_state(native,copy.state) && !raw_writers.load() &&
        raw_revision.load()==revision && polls.load()==1 && poll_generation.load()==generation;
    bool armed=false,unsafe_bound=false;
    {
        std::lock_guard lock(worker_guard);
        if (future.facts.cpu.source==copy.facts.cpu.source) {
            const bool predecessor_safe=WorkerTicket::poll_safe(current_render.binding.source,current_render.closed,current_render.sealed);
            unsafe_bound=current_render.key==copy.facts.predecessor && !predecessor_safe;
            if (native_valid && current_render.key==copy.facts.predecessor && !current_render.invalid &&
                future.facts.arm(proof,generation,revision,true,predecessor_safe)) armed=true;
            else future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
        }
    }
    if (unsafe_bound) invalidate_worker(RSF_GAME_CPU_ABORT_STATE_CHANGED,true);
    if (!armed) {
        const auto mask=predicate_bits(std::array<bool,4>{sent,native_valid,!unsafe_bound,armed});
        gate_receipt(3,mask,0xfu,after,proof,copy.facts.cpu);
    }
    return true;
}
void sample_worker_poll(uint64_t generation, uint64_t began) {
    const auto f=future_copy();
    if (!worker_source || f.facts.cpu.source!=worker_source) return;
    const auto c=render_copy(); auto proof=f.facts.predecessor; NativeState native{}; uint32_t cursor{};
    const bool valid=enabled.load() && native_cycle(proof,native,c.joining,CyclePhase::sampled) && proof==f.facts.predecessor &&
        same_state(native,f.state) && polls.load()==1 && !raw_writers.load() &&
        read(native.queue+0x1490,cursor);
    Future sampled{}; bool send=false;
    {
        std::lock_guard lock(worker_guard);
        if (future.facts.cpu.source!=worker_source) return;
        if (valid && current_render.key==f.facts.predecessor && !current_render.invalid &&
            future.facts.sample(generation,raw_revision.load(),cursor,began,poll_generation.load()==generation)) {
            sampled=future; send=true;
        } else future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
    }
    if (send && !source_fact(ticket_pending(sampled),RSF_GAME_CPU_SOURCE_SAMPLED))
        invalidate_worker(RSF_GAME_CPU_ABORT_STATE_CHANGED,false);
}
struct PhaseChecks {
    uint32_t reached{}, passed{}, queue{};
    uint32_t native_result{~uint32_t(0)}, native_mode{~uint32_t(0)};
    bool result_known{}, mode_known{};
    uint64_t generation{};
    bool test(unsigned bit, bool value) noexcept {
        reached|=uint32_t(1)<<bit;
        if (value) passed|=uint32_t(1)<<bit;
        return value;
    }
};
void downstream_receipt(const char* phase, const Future& f, const WorkerCycle& proof,
    const PhaseChecks& checks, uint32_t cursor) noexcept {
    if (!f.facts.active() || f.facts.refusal || !host.log || downstream_receipts.fetch_add(1)>=32) return;
    char text[1400]{};
    std::snprintf(text,sizeof(text),
        "Fallout4 CPU downstream candidate/refusal phase=%s source=%llu ticket_phase=%u predecessor=%llu proof_source=%llu native=%u/%u idle=%llu/%llu tls_idle=%llu generation=%llu/%llu cursor=%u/%u queue=%u reached=0x%x passed=0x%x busy=%u retiring=%u main=%p/%p window=%p/%p worker=%p/%p manager=%p/%p queue_owner=%p/%p receiver=%p/%p events=%p,%p/%p,%p session=%llu/%llu wait_result_known=%u wait_result=0x%x mode_known=%u mode=%u callback_inflight=%u callback_returned=%u native_poll_started=%u reserve_announced=%u.",
        phase,static_cast<unsigned long long>(f.facts.cpu.source),static_cast<unsigned>(f.facts.phase),
        static_cast<unsigned long long>(f.facts.predecessor.source),static_cast<unsigned long long>(proof.source),
        f.facts.predecessor.frame,proof.frame,static_cast<unsigned long long>(f.facts.predecessor.idle),
        static_cast<unsigned long long>(proof.idle),static_cast<unsigned long long>(idle_serial),
        static_cast<unsigned long long>(f.facts.generation),static_cast<unsigned long long>(checks.generation),
        f.facts.cursor,cursor,checks.queue,checks.reached,checks.passed,unsigned(f.busy),unsigned(f.retiring),
        reinterpret_cast<void*>(f.facts.predecessor.main),reinterpret_cast<void*>(proof.main),
        reinterpret_cast<void*>(f.facts.predecessor.window),reinterpret_cast<void*>(proof.window),
        reinterpret_cast<void*>(f.facts.predecessor.worker),reinterpret_cast<void*>(proof.worker),
        reinterpret_cast<void*>(f.facts.predecessor.manager),reinterpret_cast<void*>(proof.manager),
        reinterpret_cast<void*>(f.facts.predecessor.queue),reinterpret_cast<void*>(proof.queue),
        reinterpret_cast<void*>(f.facts.predecessor.receiver),reinterpret_cast<void*>(proof.receiver),
        reinterpret_cast<void*>(f.facts.predecessor.start_event),reinterpret_cast<void*>(f.facts.predecessor.complete_event),
        reinterpret_cast<void*>(proof.start_event),reinterpret_cast<void*>(proof.complete_event),
        static_cast<unsigned long long>(f.facts.predecessor.session),static_cast<unsigned long long>(proof.session),
        unsigned(checks.result_known),checks.native_result,unsigned(checks.mode_known),checks.native_mode,
        unsigned(f.facts.callback_inflight),unsigned(f.facts.callback_returned),unsigned(f.facts.native_poll_started),unsigned(f.reserve_announced));
    try { host.log(host.user,text); } catch (...) {}
}
bool worker_consumer_proof(const Future& f, WorkerCycle& proof, uint32_t& cursor, bool before_bind, PhaseChecks* receipt = nullptr, CyclePhase phase=CyclePhase::consumer_begin) {
    const auto c=render_copy(); NativeState native{}; PhaseChecks ignored{};
    auto& trace=receipt ? *receipt : ignored;
    proof=f.facts.predecessor;
    return trace.test(0,enabled.load()) && trace.test(1,native_cycle(proof,native,c.joining,phase)) &&
        trace.test(2,proof==f.facts.predecessor) && trace.test(3,same_state(native,f.state)) &&
        trace.test(4,!polls.load()) && trace.test(5,(trace.generation=poll_generation.load())==f.facts.generation) &&
        trace.test(6,(!before_bind || (!raw_writers.load() && raw_revision.load()==f.facts.raw))) &&
        trace.test(7,read(native.queue+0x1490,trace.queue)) && trace.test(8,trace.queue==f.facts.cursor) &&
        trace.test(9,read(native.receiver+8,cursor)) && trace.test(10,cursor==trace.queue);
}
bool begin_worker_consumer(void* receiver, uintptr_t caller) {
    const auto f=future_copy();
    if (!f.facts.active()) {
        const auto c=render_copy();
        if (c.binding.source && !c.closed) invalidate_worker(RSF_GAME_CPU_ABORT_STATE_CHANGED,true);
        return false;
    }
    const bool selected=job_selected && wrapper_selected && worker_source==f.facts.cpu.source &&
        delivery.queue==f.facts.predecessor.queue && delivery.receiver==uintptr_t(receiver) && caller==image()+0x1673b74;
    WorkerCycle proof{}; uint32_t cursor{}; const auto now=qpc();
    const bool valid=selected && worker_consumer_proof(f,proof,cursor,true);
    Future begun{};
    {
        std::lock_guard lock(worker_guard);
        if (future.facts.cpu.source!=f.facts.cpu.source) return false;
        if (!valid || current_render.key!=f.facts.predecessor || current_render.invalid ||
            !future.facts.begin(now,poll_generation.load(),raw_revision.load(),cursor,true)) {
            future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
        } else begun=future;
    }
    if (!begun.facts.frame_begun) { invalidate_worker(RSF_GAME_CPU_ABORT_STATE_CHANGED,true); return false; }
    const bool frame_sent=emit_identity(begun.facts.cpu,RSF_GAME_CPU_FRAME_BEGIN,0);
    const auto check=future_copy();
    if (!frame_sent || !worker_consumer_proof(check,proof,cursor,true,nullptr,CyclePhase::frame_return)) {
        invalidate_worker(RSF_GAME_CPU_ABORT_STATE_CHANGED,false); return false;
    }
    const bool bound_sent=source_fact(ticket_pending(check),RSF_GAME_CPU_SOURCE_BIND);
    PhaseChecks trace{};
    const bool still_valid=trace.test(11,bound_sent) && worker_consumer_proof(check,proof,cursor,true,&trace,CyclePhase::bind_return);
    bool refused=false;
    {
        std::lock_guard lock(worker_guard);
        if (future.facts.cpu.source==begun.facts.cpu.source &&
            (!still_valid || !trace.test(12,current_render.key==begun.facts.predecessor) || !trace.test(13,!current_render.invalid) ||
             !trace.test(14,future.facts.bind((trace.generation=poll_generation.load()),raw_revision.load(),cursor,true)))) {
            refused=!future.facts.refusal;
            future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
        }
    }
    if (refused) downstream_receipt("bind-return",check,proof,trace,cursor);
    return true;
}
void end_worker_consumer() {
    const auto f=future_copy(); WorkerCycle proof{}; uint32_t cursor{};
    if (!worker_source || f.facts.cpu.source!=worker_source) return;
    PhaseChecks trace{};
    const bool valid=worker_consumer_proof(f,proof,cursor,false,&trace,CyclePhase::consumer_return);
    bool refused=false;
    {
        std::lock_guard lock(worker_guard);
        if (future.facts.cpu.source==worker_source &&
            (!valid || !trace.test(11,future.facts.consumed((trace.generation=poll_generation.load()),cursor,true)))) {
            refused=!future.facts.refusal;
            future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
        }
    }
    if (refused) downstream_receipt("consumer-return",f,proof,trace,cursor);
}
void observe_poll(void* manager, float delta) {
    entered.fetch_add(1, std::memory_order_acq_rel);
#if defined(_MSC_VER)
    const auto caller = reinterpret_cast<uintptr_t>(_ReturnAddress());
#else
    const auto caller = reinterpret_cast<uintptr_t>(__builtin_return_address(0));
#endif
    const auto error = GetLastError();
    uint32_t previous_polls{}; uint64_t generation{};
    const bool mapped=reserve_worker_poll(manager,caller,generation,previous_polls);
    if (pending.source) abort_pending(previous_polls ? RSF_GAME_CPU_ABORT_OVERLAP : RSF_GAME_CPU_ABORT_INTERVENING_POLL);
    NativeState before{};
    StateRead state_read=StateRead::not_called;
    if (!mapped) {
        const auto current=render_copy(); auto proof=current.key; NativeState native{};
        const bool poll_only=enabled.load() && !previous_polls && !raw_writers.load() &&
            native_cycle(proof,native,false) && proof==current.key && same_state(native,current.state);
        invalidate_worker(previous_polls ? RSF_GAME_CPU_ABORT_OVERLAP : RSF_GAME_CPU_ABORT_INTERVENING_POLL,true,poll_only ? &current.key : nullptr);
    }
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
    if (mapped) {
        bool unsafe=false;
        {
            std::lock_guard lock(worker_guard);
            if (future.facts.cpu.source==worker_source) {
                unsafe=!enabled.load() || current_render.key!=future.facts.predecessor || current_render.invalid ||
                    polls.load()!=1 || poll_generation.load()!=generation || raw_writers.load() ||
                    raw_revision.load()!=future.facts.raw ||
                    !WorkerTicket::poll_safe(current_render.binding.source,current_render.closed,current_render.sealed);
                if (unsafe) future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
            }
        }
        if (unsafe) invalidate_worker(RSF_GAME_CPU_ABORT_STATE_CHANGED,true);
        {
            std::lock_guard lock(worker_guard);
            if (future.facts.cpu.source==worker_source) future.facts.native_poll_entry();
        }
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
    if (mapped) sample_worker_poll(generation,sample_begin);
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
    const bool worker_consuming=begin_worker_consumer(receiver,caller);
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
    if (worker_consuming) end_worker_consumer();
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
    delivery = (caller == image() + 0xc342de || (wrapper_selected && caller==image()+0xc34eec)) && delivery_current(queue, receiver) ?
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
        const auto f=future_copy(); auto proof=f.facts.predecessor; NativeState native{}; uint32_t cursor{},sequence{};
        proof.source=f.facts.cpu.source; proof.idle=idle_serial;
        PhaseChecks trace{};
        const bool valid=trace.test(0,f.facts.active()) && trace.test(1,enabled.load()) && trace.test(2,native_cycle(proof,native,false,CyclePhase::swap_claim)) &&
            trace.test(3,proof.main==uintptr_t(main)) && trace.test(4,same_state(native,f.state)) && trace.test(5,read(native.receiver+8,cursor)) &&
            trace.test(6,read(native.queue+0x1490,sequence)) && trace.test(7,sequence==cursor) && trace.test(8,!polls.load());
        trace.queue=sequence;
        bool claimed=false,refused=false;
        if (f.facts.active()) {
            std::lock_guard lock(worker_guard);
            if (trace.test(9,!future.busy) && trace.test(10,!future.retiring) && trace.test(11,future.facts.cpu.source==f.facts.cpu.source) && valid &&
                trace.test(12,future.facts.claim(proof,cursor,(trace.generation=poll_generation.load())))) {
                identity=future.facts.cpu; rendered=0; future_claimed=true; claimed=true;
            } else { refused=!future.facts.refusal; future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED); }
        }
        if (refused) downstream_receipt("swap-claim",f,proof,trace,cursor);
        if (!claimed) (void)drain_future();
    }
    if (!identity.source && !future_copy().facts.active()) {
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
    if (future_claimed) {
        invalidate_worker(RSF_GAME_CPU_ABORT_NOT_CONSUMED,false);
        identity={}; rendered=0; future_claimed=false;
    } else close_frame();
    { std::lock_guard lock(worker_guard); if (current_render.cpu.source && current_render.key.idle==idle_serial) current_render.swapped=true; }
    pending = {};
    SetLastError(native_error);
}
void observe_job_end(void* argument) {
    const auto error=GetLastError(); entered.fetch_add(1,std::memory_order_acq_rel);
#if defined(_MSC_VER)
    const auto caller=uintptr_t(_ReturnAddress());
#else
    const auto caller=uintptr_t(__builtin_return_address(0));
#endif
    const auto previous=jobs.fetch_add(1); ++job_depth;
    const auto parent=job_selected; const auto parent_source=worker_source;
    const auto c=render_copy(); auto proof=c.key; NativeState native{};
    job_selected=enabled.load() && !previous && job_depth==1 && caller==image()+0xbd76df &&
        native_cycle(proof,native,false) && proof==c.key;
    worker_source=0;
    SetLastError(error); original_job_end(argument); const auto native_error=GetLastError();
    const auto f=future_copy();
    if (worker_source && f.facts.cpu.source==worker_source) {
        const auto current=render_copy(); proof=f.facts.predecessor; uint32_t cursor{}; PhaseChecks trace{};
        const bool valid=trace.test(0,enabled.load()) && trace.test(1,native_cycle(proof,native,current.joining,CyclePhase::job_complete)) &&
            trace.test(2,proof==f.facts.predecessor) && trace.test(3,same_state(native,f.state)) &&
            trace.test(4,(trace.generation=poll_generation.load())==f.facts.generation) &&
            trace.test(5,read(native.receiver+8,cursor)) && trace.test(6,cursor==f.facts.cursor);
        bool refused=false;
        {
            std::lock_guard lock(worker_guard);
            if (future.facts.cpu.source==worker_source) {
                if (!valid || !trace.test(7,future.facts.complete(true))) {
                    refused=!future.facts.refusal;
                    future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
                }
                future.busy=false;
            }
        }
        if (refused) downstream_receipt("job-return",f,proof,trace,cursor);
        (void)drain_future();
    }
    worker_source=parent_source; job_selected=parent; --job_depth; jobs.fetch_sub(1);
    entered.fetch_sub(1,std::memory_order_acq_rel); SetLastError(native_error);
}
void observe_wrapper(void* main) {
    const auto error=GetLastError(); entered.fetch_add(1,std::memory_order_acq_rel);
#if defined(_MSC_VER)
    const auto caller=uintptr_t(_ReturnAddress());
#else
    const auto caller=uintptr_t(__builtin_return_address(0));
#endif
    const auto parent=wrapper_selected; ++wrapper_depth;
    uintptr_t actual_main{};
    wrapper_selected=job_selected && wrapper_depth==1 && caller==image()+0xbd6945 &&
        read(image()+0x32dd130,actual_main) && actual_main==uintptr_t(main);
    SetLastError(error); original_wrapper(main); const auto native_error=GetLastError();
    wrapper_selected=parent; --wrapper_depth;
    entered.fetch_sub(1,std::memory_order_acq_rel); SetLastError(native_error);
}
uint32_t observe_worker_join() {
    const auto error=GetLastError(); entered.fetch_add(1,std::memory_order_acq_rel);
#if defined(_MSC_VER)
    const auto caller=uintptr_t(_ReturnAddress());
#else
    const auto caller=uintptr_t(__builtin_return_address(0));
#endif
    const auto c=render_copy(); auto proof=c.key; NativeState native{};
    PhaseChecks entry_trace{};
    const bool selected=entry_trace.test(0,enabled.load()) && entry_trace.test(1,caller==image()+0xc346c0) &&
        entry_trace.test(2,idle_depth==1) && entry_trace.test(3,!swap_depth) && entry_trace.test(4,c.swapped) &&
        entry_trace.test(5,c.closed) && entry_trace.test(6,c.tid==GetCurrentThreadId()) &&
        entry_trace.test(7,c.key.main==uintptr_t(idle_main)) && entry_trace.test(8,c.key.idle==idle_serial) &&
        entry_trace.test(9,native_cycle(proof,native,false,CyclePhase::join_entry)) && entry_trace.test(10,proof==c.key);
    const auto entry_proof=proof;
    if (selected) { std::lock_guard lock(worker_guard); if (current_render.key==c.key) current_render.joining=true; }
    SetLastError(error); const auto result=original_worker_join(); const auto native_error=GetLastError();
    const auto f=future_copy(); proof=c.key; uint32_t cursor{},sequence{},mode{};
    // Early/no-op native RET leaves EAX undefined. Only selected proves the
    // nonnull mode1 branch that actually tail-calls WaitForSingleObjectEx.
    PhaseChecks trace{};
    const bool valid=trace.test(0,selected) && trace.test(1,enabled.load()) && trace.test(2,result==WAIT_OBJECT_0) &&
        trace.test(3,native_cycle(proof,native,true,CyclePhase::join_return)) && trace.test(4,proof==c.key) && trace.test(5,read(proof.worker+0x68,mode)) &&
        trace.test(6,mode==0) && trace.test(7,same_state(native,f.state)) && trace.test(8,read(native.receiver+8,cursor)) &&
        trace.test(9,cursor==f.facts.cursor) && trace.test(10,read(native.queue+0x1490,sequence)) && trace.test(11,sequence==cursor) &&
        trace.test(12,(trace.generation=poll_generation.load())==f.facts.generation);
    trace.queue=sequence;
    trace.result_known=selected;
    if (trace.result_known) trace.native_result=result;
    trace.mode_known=(trace.passed & (uint32_t(1)<<5))!=0;
    if (trace.mode_known) trace.native_mode=mode;
    bool refused=false;
    {
        std::lock_guard lock(worker_guard);
        if (selected && current_render.key==c.key) {
            if (future.facts.active() && (!valid || !trace.test(13,!future.busy) ||
                !trace.test(14,future.facts.join(proof,true)))) {
                refused=!future.facts.refusal;
                future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
            }
            current_render.joining=false;
        }
    }
    if (!selected) downstream_receipt("join-unselected",f,entry_proof,entry_trace,0);
    else if (refused) downstream_receipt("join-return",f,proof,trace,cursor);
    (void)drain_future();
    entered.fetch_sub(1,std::memory_order_acq_rel); SetLastError(native_error); return result;
}
struct Hook { uintptr_t rva; void* replacement; void** trampoline; };
std::array<Hook, 10> hooks() {
    return {{{0xc33250, reinterpret_cast<void*>(&observe_swap), reinterpret_cast<void**>(&original)},
        {0x1675140, reinterpret_cast<void*>(&observe_keyboard), reinterpret_cast<void**>(&original_keyboard)},
        {0x16754d0, reinterpret_cast<void*>(&observe_mouse), reinterpret_cast<void**>(&original_mouse)},
        {0xc339d0, reinterpret_cast<void*>(&observe_idle), reinterpret_cast<void**>(&original_idle)},
        {0x1672540, reinterpret_cast<void*>(&observe_poll), reinterpret_cast<void**>(&original_poll)},
        {0x1673ae0, reinterpret_cast<void*>(&observe_consume), reinterpret_cast<void**>(&original_consume)},
        {0xddbe50, reinterpret_cast<void*>(&observe_player_controls), reinterpret_cast<void**>(&original_player_controls)},
        {0xbd6850, reinterpret_cast<void*>(&observe_job_end), reinterpret_cast<void**>(&original_job_end)},
        {0xc34eb0, reinterpret_cast<void*>(&observe_wrapper), reinterpret_cast<void**>(&original_wrapper)},
        {0xbd51c0, reinterpret_cast<void*>(&observe_worker_join), reinterpret_cast<void**>(&original_worker_join)}}};
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
void quiesce_cpu_events() { enabled.store(false, std::memory_order_release); invalidate_worker(RSF_GAME_CPU_ABORT_QUIESCE,true); }
bool cpu_events_busy() { return entered.load(std::memory_order_acquire) != 0; }
bool stop_cpu_events() {
    quiesce_cpu_events();
    if (!retire_peek()) return false;
    if (cpu_events_busy()) return false;
    invalidate_worker(RSF_GAME_CPU_ABORT_QUIESCE,true);
    if (!drain_future() || !drain_current_abort()) return false;
    for (const auto& hook : hooks()) {
        if (!*hook.trampoline) continue;
        const auto address = reinterpret_cast<void*>(image() + hook.rva);
        const auto result = MH_DisableHook(address);
        if (result != MH_OK && result != MH_ERROR_DISABLED) return false;
        if (cpu_events_busy() || MH_RemoveHook(address) != MH_OK) return false;
        *hook.trampoline = nullptr;
    }
    { std::lock_guard lock(worker_guard); current_render={}; }
    return true;
}
CpuIdentity current_cpu_identity() noexcept {
    return swap_admitted && swap_depth == 1 && idle_depth <= 1 ? identity : CpuIdentity{};
}
bool cpu_source_is_current(const CpuIdentity& expected) noexcept {
    if (!enabled.load() || !swap_admitted || swap_depth!=1 || idle_depth>1 || source_invalid || future_claimed ||
        (pending.source && invalid_pending()) || rendered!=1 || !expected.source || !expected.submission ||
        expected.session!=identity.session || expected.source!=identity.source || expected.submission!=identity.submission ||
        expected.began_qpc!=identity.began_qpc || expected.frequency!=identity.frequency) return false;
    const auto c=render_copy();
    if (c.cpu.source!=identity.source) return false;
    if (c.invalid) return false;
    if (c.binding.source && !bound_current_valid(c,false)) return false;
    return true;
}
bool begin_cpu_render() noexcept {
    if (!enabled.load() || !swap_admitted || swap_depth!=1 || idle_depth>1 || !identity.source || source_invalid || rendered ||
        (pending.source && invalid_pending()) || !drain_current_abort()) return false;
    auto f=future_copy(); WorkerCycle proof{}; NativeState native{}; uint32_t cursor{},sequence{};
    proof.session=identity.session; proof.source=identity.source; proof.idle=idle_serial; proof.main=uintptr_t(idle_main);
    const bool native_valid=native_cycle(proof,native,false,CyclePhase::world_adopt);
    PhaseChecks trace{};
    const bool adoption_valid=trace.test(0,future_claimed) && trace.test(1,native_valid) && trace.test(2,same_state(native,f.state)) &&
        trace.test(3,!polls.load()) && trace.test(4,read(native.receiver+8,cursor)) && trace.test(5,read(native.queue+0x1490,sequence)) && trace.test(6,sequence==cursor);
    trace.queue=sequence;
    const auto tid=GetCurrentThreadId();
    bool failed=false,refused=false;
    {
        std::lock_guard lock(worker_guard);
        if (current_render.aborting || current_render.abort_pending) return false;
        Pending binding{};
        if (future_claimed) {
            if (!adoption_valid || !trace.test(7,future.facts.cpu.source==identity.source) || !trace.test(8,!future.busy) || !trace.test(9,!future.retiring) ||
                !trace.test(10,future.facts.adopt(proof,cursor,(trace.generation=poll_generation.load())))) {
                refused=!future.facts.refusal;
                future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED); failed=true;
            } else { binding=ticket_pending(future); future={}; future_claimed=false; }
        } else if (pending.bound && pending.source==identity.source) binding=pending;
        if (!failed) {
            identity.submission=next_submission.fetch_add(1); rendered=1;
            current_render={}; current_render.key=proof; current_render.cpu=identity; current_render.state=native;
            current_render.binding=binding; current_render.tid=tid; current_render.constructed=native_valid;
        }
    }
    if (refused) downstream_receipt("world-adopt",f,proof,trace,cursor);
    if (failed) { (void)drain_future(); source_invalid=true; return false; }
    return true;
}
bool begin_cpu_present_phase(const CpuIdentity& source, uint64_t native_frame, uint64_t window) noexcept {
    const bool source_current=cpu_source_is_current(source);
    const auto c=render_copy(); auto proof=c.key; NativeState native{};
    const bool native_valid=native_cycle(proof,native,false,CyclePhase::present);
    const auto bits=predicate_bits(std::array<bool,11>{enabled.load(),source_current,c.constructed,!c.closed,!c.present,
        native_frame==c.key.frame,window==c.key.window,c.tid==GetCurrentThreadId(),native_valid,proof==c.key,same_state(native,c.state)});
    if (bits!=0x7ffu) { gate_receipt(2,bits,0x7ffu,c,proof,source); return false; }
    std::lock_guard lock(worker_guard);
    if (current_render.key!=c.key || current_render.invalid || current_render.closed) return false;
    current_render.sealed=current_render.present=true;
    return true;
}
void end_cpu_present_phase(const CpuIdentity& source) noexcept {
    std::lock_guard lock(worker_guard);
    if (current_render.cpu.session==source.session && current_render.cpu.source==source.source)
        current_render.present=false;
}
}
