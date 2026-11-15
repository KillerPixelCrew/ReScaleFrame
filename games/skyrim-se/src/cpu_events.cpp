// SPDX-License-Identifier: GPL-3.0-only
#include "cpu_events.h"
#include "input_ownership.h"
#include "screen_classification.h"
#include <MinHook.h>
#include <windows.h>
#include <array>
#include <atomic>
#include <cstring>
#include <cstdio>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

namespace rsf::skyrim {
namespace {
struct Frame {
    rsf_game_host_services host{};
    uint64_t source{};
    uint64_t render_scope{};
    uint32_t primary_count{};
    bool sampled{}, render_entered{}, simulation_ended{};
    InputOwnership::Stamp consumed{};
    uint32_t camera_updates{};
    bool input_ambiguous{};
    rsf_screen_class screen{RSF_SCREEN_UNKNOWN};
    uint32_t primary_world_entries{};
    NativeJobOwnership::Ticket jobs{};
    void* main{};
    uint32_t movement_calls{}, player_update_calls{};
    uint32_t player_camera_updates{}; // TESCamera::Update 0x558e70 on PlayerCamera, any state
    ProspectiveSources::Reservation reservation{}; // adopted at tick entry, if any
    bool entry_readable{}, queued_move{}; // player queued-move gates read at tick entry
    BindRefusal refusal{BindRefusal::none};
};
thread_local Frame* current{};
std::atomic<uint64_t> next_source{1};
std::atomic<uint64_t> next_render_scope{1};
std::atomic<uint32_t> entered{};
std::atomic<bool> active{};
std::atomic<uint32_t> poll_samples{};
rsf_game_host_services services{};
uint64_t frequency{};
InputOwnership input_ownership;
NativeJobOwnership job_ownership;
thread_local NativeJobOwnership::Ticket named_poll_jobs{};
struct PollScope { InputOwnership::Poll ticket{}; void* manager{}; bool produced{}; uint32_t kinds{}; };
thread_local PollScope* current_poll{};
thread_local void* dispatched_list{};
ProspectiveSources prospective;
// Diagnostic call envelopes inside the bound source's own native job generation, on any
// thread. They do not prove complete movement/derived-state coverage and are never reported
// to the runtime as consumption.
struct WorkerConsumption {
    std::atomic<uint64_t> source{};
    std::atomic<uint32_t> movement{}, player_update{}, camera{}, mismatched{};
} worker_consumption;
std::atomic<uint32_t> prospective_reports{};
struct Entry { Entry() { ++entered; } ~Entry() { --entered; } };
// Version 2 source lifetime fact. The runtime decides what, if anything, it means.
void emit_source(const rsf_game_host_services& host, uint32_t stage, uint64_t source, uint64_t predecessor,
    uint64_t generation, uint32_t flags, uint32_t kinds, uint32_t reason, uint64_t poll_begin = 0)
{
    if (!host.cpu_event || !source) return;
    LARGE_INTEGER now{}; QueryPerformanceCounter(&now);
    rsf_game_cpu_source_event event{};
    event.header.struct_size = sizeof(event); event.header.stage = stage; event.header.session_id = host.session_id;
    event.header.source_frame_id = source; event.header.timestamp_qpc = uint64_t(now.QuadPart);
    event.header.qpc_frequency = frequency; event.header.event_version = RSF_GAME_CPU_EVENT_VERSION_SOURCE;
    event.header.flags = flags; event.header.input_kind = kinds;
    event.predecessor_source_id = predecessor; event.native_generation = generation; event.reason = reason;
    event.poll_begin_qpc = poll_begin;
    try { host.cpu_event(host.user, &event.header); } catch (...) {}
}
void abort_source(const rsf_game_host_services& host, uint64_t source, uint32_t reason)
{
    emit_source(host, RSF_GAME_CPU_SOURCE_ABORT, source, 0, 0, 0, 0, reason);
}
void emit(Frame& frame, uint32_t stage)
{
    if (!frame.host.cpu_event) return;
    LARGE_INTEGER now{}; QueryPerformanceCounter(&now);
    rsf_game_cpu_event event{};
    event.struct_size = sizeof(event); event.stage = stage; event.session_id = frame.host.session_id;
    event.source_frame_id = frame.source; event.timestamp_qpc = uint64_t(now.QuadPart);
    event.qpc_frequency = frequency; event.event_version = RSF_GAME_CPU_EVENT_VERSION;
    event.flags = RSF_GAME_CPU_INPUT_KEYED;
    if (stage == RSF_GAME_CPU_FRAME_END && !frame.render_entered) event.flags |= RSF_GAME_CPU_RENDER_SKIPPED;
    event.render_expected = frame.primary_count;
    try { frame.host.cpu_event(frame.host.user, &event); } catch (...) {}
}
short tick(void*);
// Native second argument is the device timestep in XMM1, not an unused register.
uintptr_t poll(void*, float);
uintptr_t render(void*);
uintptr_t world(void*, uint32_t, uint8_t);
int dispatch(void*, void*);
uintptr_t controls(void*, void*, void*);
uintptr_t camera_update(void*);
uint32_t jobs_submit();
uint32_t jobs_join();
uintptr_t named_poll(void*);
void movement(void*, float, void*, float*);
uintptr_t player_update(void*, float);
void player_camera_update(void*);
struct Hook {
    uint32_t rva;
    std::array<unsigned char, 19> bytes;
    void* detour;
    void* original{};
    void* target{};
    bool created{}, enabled{};
};
std::array<Hook, 13> hooks{{
    {0x658870, {0x48,0x8b,0xc4,0x55,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57,0x48,0x81,0xec,0x30,0x01,0x00,0x00}, reinterpret_cast<void*>(&tick)},
    {0xcf95f0, {0x48,0x89,0x5c,0x24,0x10,0x48,0x89,0x74,0x24,0x18,0x57,0x48,0x83,0xec,0x30,0x0f,0x29,0x74,0x24}, reinterpret_cast<void*>(&poll)},
    {0x6565b0, {0x40,0x53,0x55,0x56,0x57,0x41,0x54,0x41,0x56,0x41,0x57,0x48,0x81,0xec,0x60,0x01,0x00,0x00,0x48}, reinterpret_cast<void*>(&render)},
    {0xcfa650, {0x48,0x8b,0xc4,0x48,0x89,0x50,0x10,0x55,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57,0x48}, reinterpret_cast<void*>(&dispatch)},
    {0x656e60, {0x48,0x8b,0xc4,0x89,0x50,0x10,0x55,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57,0x48,0x8d}, reinterpret_cast<void*>(&world)},
    {0x7ad900, {0x48,0x8b,0xc4,0x56,0x57,0x41,0x54,0x41,0x56,0x41,0x57,0x48,0x81,0xec,0x80,0x00,0x00,0x00,0x48}, reinterpret_cast<void*>(&controls)},
    {0x8fd590, {0x40,0x57,0x48,0x83,0xec,0x70,0x48,0xc7,0x44,0x24,0x20,0xfe,0xff,0xff,0xff,0x48,0x89,0x9c,0x24}, reinterpret_cast<void*>(&camera_update)},
    {0x60eaf0, {0x48,0x83,0xec,0x28,0x48,0x83,0x3d,0x94,0x1f,0xc1,0x02,0x00,0x74,0x69,0x48,0x89,0x5c,0x24,0x30}, reinterpret_cast<void*>(&jobs_submit)},
    {0x60eb80, {0x40,0x53,0x48,0x83,0xec,0x20,0x48,0x8b,0x1d,0x03,0x1f,0xc1,0x02,0x48,0x85,0xdb,0x74,0x26,0x83}, reinterpret_cast<void*>(&jobs_join)},
    {0x6e6030, {0x48,0x8b,0x0d,0xf9,0xa4,0xb4,0x02,0xe9,0xb4,0x3d,0xf7,0xff,0xcc,0xcc,0xcc,0xcc,0xcc,0xcc,0xcc}, reinterpret_cast<void*>(&named_poll)},
    {0x776290, {0x48,0x8b,0xc4,0x41,0x56,0x48,0x81,0xec,0xe0,0x00,0x00,0x00,0x48,0xc7,0x44,0x24,0x30,0xfe,0xff}, reinterpret_cast<void*>(&movement)},
    {0x745200, {0x48,0x8b,0xc4,0x55,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57,0x48,0x8d,0xa8,0x98,0xfe}, reinterpret_cast<void*>(&player_update)},
    // TESCamera::Update(this) -> current state vfunc +0x18; void, RCX only.
    {0x558e70, {0x40,0x57,0x48,0x83,0xec,0x30,0x48,0xc7,0x44,0x24,0x20,0xfe,0xff,0xff,0xff,0x48,0x89,0x5c,0x24}, reinterpret_cast<void*>(&player_camera_update)}
}};
struct NativeQueryGuard { uint32_t rva; std::array<unsigned char, 19> bytes; };
constexpr std::array<NativeQueryGuard, 2> screen_query_guards{{
    {0x1169530, {0x40,0x53,0x48,0x83,0xec,0x40,0x48,0xc7,0x44,0x24,0x20,0xfe,0xff,0xff,0xff,0x33,0xc0,0x48,0x89}},
    {0x1169590, {0x48,0x89,0x5c,0x24,0x10,0x48,0x89,0x6c,0x24,0x18,0x48,0x89,0x74,0x24,0x20,0x57,0x41,0x56,0x41}}
}};
bool copy_input(void* out, const void* pointer, size_t size) noexcept {
#if defined(_MSC_VER)
    __try { std::memcpy(out, pointer, size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION memory{};
    const auto address = uintptr_t(pointer);
    if (!VirtualQuery(pointer, &memory, sizeof(memory)) || memory.State != MEM_COMMIT ||
        (memory.Protect & (PAGE_NOACCESS | PAGE_GUARD)) || address + size < address ||
        address + size > uintptr_t(memory.BaseAddress) + memory.RegionSize) return false;
    std::memcpy(out, pointer, size); return true;
#endif
}
struct ManagerState {
    uintptr_t manager{}, main{};
    uint32_t requested{}, running{};
    bool read() noexcept {
        const auto base = uintptr_t(GetModuleHandleW(nullptr));
        const auto load = [](auto& value, uintptr_t address) {
            return copy_input(&value, reinterpret_cast<const void*>(address), sizeof(value));
        };
        return load(manager, base + 0x3220a90) && manager && load(main, base + 0x3230530) && main &&
            load(requested, manager + 0x68) && load(running, manager + 0x6c);
    }
};
uint32_t jobs_submit()
{
    Entry lease;
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    ManagerState before{};
    NativeJobOwnership::Ticket ticket{};
    if (active.load(std::memory_order_acquire)) {
        if (current && caller == base + 0x658b14 && before.read() && before.main == uintptr_t(current->main) &&
            before.requested == 0 && before.running == 0) {
            // Main reaches this call only on its normal-simulation branch (0x658aed..0x658b0f),
            // after the pause/freeze decision and before any worker of this generation runs.
            // Bind the adopted poll here only while its completed control state is unchanged.
            // This is ownership of that poll, not evidence that earlier main-thread work
            // (0x743090 at 0x6588de) or later worker consumers were covered.
            if (current->reservation.source) {
                // 0x659ea0 runs next, at 0x658b17: free camera there bypasses the 0x745200 envelope.
                uintptr_t camera{}, state{}, free_state{};
                const bool camera_read = copy_input(&camera, reinterpret_cast<const void*>(base + 0x31a5478), sizeof(camera)) &&
                    camera && copy_input(&state, reinterpret_cast<const void*>(camera + 0x28), sizeof(state)) &&
                    copy_input(&free_state, reinterpret_cast<const void*>(camera + 0xd0), sizeof(free_state));
                current->refusal = bind_refusal(current->entry_readable && camera_read, current->queued_move,
                    current->player_update_calls || current->movement_calls || current->camera_updates ||
                        current->player_camera_updates,
                    state == free_state); // the native test, including both null
                const auto stamp = input_ownership.snapshot();
                if (current->refusal != BindRefusal::none) {
                    if (prospective.release(current->source).source) {
                        current->reservation.phase = ProspectiveSources::none;
                        abort_source(current->host, current->source, RSF_GAME_CPU_ABORT_NOT_CONSUMED);
                    }
                } else if (prospective.bind(current->source, stamp.generation)) {
                    worker_consumption.source.store(0);
                    worker_consumption.movement.store(0); worker_consumption.player_update.store(0);
                    worker_consumption.camera.store(0); worker_consumption.mismatched.store(0);
                    worker_consumption.source.store(current->source);
                    current->reservation.phase = ProspectiveSources::bound;
                    emit_source(current->host, RSF_GAME_CPU_SOURCE_BIND, current->source, 0, 0, 0, 0, 0);
                } else if (prospective.release(current->source).source) {
                    current->reservation.phase = ProspectiveSources::none;
                    abort_source(current->host, current->source, RSF_GAME_CPU_ABORT_STATE_CHANGED);
                }
            }
            // Original wakes the manager only after setting both native mode1 fields.
            // Publish immutable scalars first so a newly awakened worker sees them.
            ticket = job_ownership.publish(before.manager, current->source, current->host.session_id);
            current->jobs = ticket;
        } else job_ownership.invalidate();
    }
    const auto result = reinterpret_cast<uint32_t (*)()>(hooks[7].original)();
    ManagerState after{};
    if (ticket.generation && (!result || !after.read() || after.manager != before.manager ||
        after.main != before.main || after.requested != 1 || after.running != 1)) job_ownership.invalidate();
    return result;
}
uint32_t jobs_join()
{
    Entry lease;
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    ManagerState before{};
    const auto ticket = current ? current->jobs : NativeJobOwnership::Ticket{};
    const bool owns_wait = current && caller == base + 0x659334 && before.read() &&
        before.main == uintptr_t(current->main) && before.manager == ticket.manager &&
        before.requested <= 1 && before.running == 1 && job_ownership.matches(ticket);
    const auto result = reinterpret_cast<uint32_t (*)()>(hooks[8].original)();
    if (owns_wait) {
        ManagerState after{};
        if (result == WAIT_OBJECT_0 && after.read() && after.manager == before.manager &&
            after.main == before.main && after.requested == 0 && after.running == 0) {
            (void)job_ownership.retire(ticket);
        } else job_ownership.invalidate();
    } else if (active.load(std::memory_order_acquire) && job_ownership.snapshot().generation) job_ownership.invalidate();
    return result;
}
uintptr_t named_poll(void* data)
{
    Entry lease;
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    const auto previous = named_poll_jobs;
    named_poll_jobs = {};
    ManagerState native{};
    const auto ticket = job_ownership.snapshot();
    if (active.load(std::memory_order_acquire) && caller == uintptr_t(GetModuleHandleW(nullptr)) + 0xebc248 &&
        !previous.generation && native.read() &&
        native.manager == ticket.manager && native.requested <= 1 && native.running == 1)
        named_poll_jobs = ticket;
    const auto result = reinterpret_cast<uintptr_t (*)(void*)>(hooks[9].original)(data);
    named_poll_jobs = previous;
    return result;
}
rsf_screen_class native_screen(void* main, bool primary_world) noexcept {
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    struct State {
        uintptr_t main{}, ui{}, names{}, player{}, manager{}, cell{}, loading_name{}, main_name{};
        uint32_t pause{}, requested{}, running{};
        uint8_t freeze{};
        bool read(uintptr_t module) noexcept {
            const auto load = [](auto& value, uintptr_t address) {
                return copy_input(&value, reinterpret_cast<const void*>(address), sizeof(value));
            };
            return load(main, module + 0x3230530) && main &&
                load(ui, module + 0x219e5c0) && ui && load(names, module + 0x21a05c8) && names &&
                load(loading_name, names + 0xe8) && loading_name && load(main_name, names + 0xa8) && main_name &&
                load(player, module + 0x3230778) && player && load(manager, module + 0x3220a90) && manager &&
                load(cell, player + 0x60) && load(pause, ui + 0x160) && load(freeze, main + 0x16) &&
                load(requested, manager + 0x68) && load(running, manager + 0x6c);
        }
        bool same(const State& other) const noexcept {
            return main == other.main && ui == other.ui && names == other.names && player == other.player &&
                manager == other.manager && cell == other.cell && pause == other.pause && freeze == other.freeze &&
                requested == other.requested && running == other.running &&
                loading_name == other.loading_name && main_name == other.main_name;
        }
    } before{}, after{};
    if (!before.read(base) || before.main != uintptr_t(main)) return RSF_SCREEN_UNKNOWN;
    // Native lookup retains/releases the menu under its own map synchronization.
    // Borrow the engine's interned BSFixedStrings; do not traverse a mutable stack.
    const auto open = reinterpret_cast<bool (*)(void*, const void*)>(base + 0x1169530);
    NativeScreenFacts facts{};
    facts.loading_menu = open(reinterpret_cast<void*>(before.ui), reinterpret_cast<const void*>(before.names + 0xe8));
    facts.main_menu = open(reinterpret_cast<void*>(before.ui), reinterpret_cast<const void*>(before.names + 0xa8));
    facts.frozen_background = reinterpret_cast<bool (*)(void*)>(base + 0x1169590)(reinterpret_cast<void*>(before.ui));
    if (!after.read(base) || !before.same(after)) return RSF_SCREEN_UNKNOWN;
    facts.readable = facts.owned_main = true;
    facts.primary_world = primary_world;
    facts.pause_count = before.pause; facts.freeze_time = before.freeze != 0;
    facts.parent_cell = before.cell != 0;
    facts.requested_mode = before.requested; facts.running_mode = before.running;
    return classify_screen(facts);
}
rsf_game_render_pass render_fact(uint32_t role) {
    rsf_game_render_pass pass{}; pass.struct_size = sizeof(pass); pass.role = role;
    if (!current || !current->host.render_pass || !cpu_render_identity(pass)) return pass;
    pass.session_id = current->host.session_id;
    pass.flags |= RSF_GAME_RENDER_PRIMARY;
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    uintptr_t window{}, root{}, camera{};
    (void)copy_input(&pass.native_frame, reinterpret_cast<const void*>(base + 0x33363f4), sizeof(uint32_t));
    if (copy_input(&root, reinterpret_cast<const void*>(base + 0x3230740), sizeof(root)) && root)
        (void)copy_input(&camera, reinterpret_cast<const void*>(root + 296), sizeof(camera));
    pass.family_key = pass.view_key = pass.history_key = camera;
    if (copy_input(&window, reinterpret_cast<const void*>(base + 0x3330198), sizeof(window)) && window) {
        pass.viewport_key = pass.window_key = pass.rhi_viewport_key = window;
        (void)copy_input(&pass.swapchain, reinterpret_cast<const void*>(window + 0x18), sizeof(pass.swapchain));
    }
    return pass;
}
void report_render(const rsf_game_render_pass& pass, uint32_t begin) {
    if (current && current->host.render_pass && pass.source_frame_id) {
        try { current->host.render_pass(current->host.user, nullptr, &pass, begin); } catch (...) {}
    }
}
uintptr_t world(void* main, uint32_t output, uint8_t flags) {
    Entry lease;
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    auto pass = render_fact(RSF_GAME_RENDER_SUBMISSION);
    const bool primary = caller == uintptr_t(GetModuleHandleW(nullptr)) + 0x656e32 && output == 0 &&
        pass.source_frame_id && pass.view_key && pass.swapchain;
    if (primary) {
        ++current->primary_world_entries;
        current->screen = native_screen(main, true);
        if (current->primary_world_entries != 1) current->screen = RSF_SCREEN_UNKNOWN;
        pass.screen = current->screen;
        (void)cpu_render_identity(pass);
        report_render(pass, 1);
    }
    const auto result = reinterpret_cast<uintptr_t (*)(void*, uint32_t, uint8_t)>(hooks[4].original)(main, output, flags);
    if (primary) {
        ++current->primary_count;
        (void)cpu_render_identity(pass);
        report_render(pass, 0);
    }
    return result;
}
uint32_t input_kinds(const void* list) noexcept {
    struct Event { uintptr_t vtable; uint32_t device, type; uintptr_t next; };
    static_assert(sizeof(Event) == 0x18);
    uintptr_t next{};
    if (!list || !copy_input(&next, list, sizeof(next))) return 0;
    uint32_t kinds{};
    // Native events are borrowed until PollDevices clears its queue after dispatch.
    // Keep only kinds, never buttons, text, deltas or native event pointers.
    for (unsigned count = 0; next && count < 128; ++count) {
        Event event{};
        if (!copy_input(&event, reinterpret_cast<const void*>(next), sizeof(event))) return 0;
        if (event.type == 0) {
            if (event.device == 0 || event.device == 3) kinds |= RSF_GAME_INPUT_KEYBOARD;
            else if (event.device == 1) kinds |= RSF_GAME_INPUT_MOUSE;
            else if (event.device == 2) kinds |= RSF_GAME_INPUT_CONTROLLER;
        } else if (event.type == 1 && event.device == 1) kinds |= RSF_GAME_INPUT_MOUSE;
        else if (event.type == 2 && (event.device == 0 || event.device == 3)) kinds |= RSF_GAME_INPUT_KEYBOARD;
        else if (event.type == 3 && event.device == 2) kinds |= RSF_GAME_INPUT_CONTROLLER;
        next = event.next;
    }
    return next ? 0 : kinds; // Cyclic or excessive lists do not publish partial activity.
}
int dispatch(void* manager, void* list) {
    Entry lease;
    rsf_game_cpu_event event{};
    rsf_game_host_services host{};
    const auto base = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr));
    uintptr_t native_manager{};
    if (active.load(std::memory_order_acquire) && copy_input(&native_manager, base + 0x3204b30, sizeof(native_manager)) &&
        uintptr_t(manager) == native_manager) {
        event.input_kind = input_kinds(list);
        if (event.input_kind) {
            host = services;
            LARGE_INTEGER now{}; QueryPerformanceCounter(&now);
            event.struct_size = sizeof(event); event.stage = RSF_GAME_CPU_INPUT_EVENT;
            event.session_id = host.session_id; event.timestamp_qpc = uint64_t(now.QuadPart);
            event.qpc_frequency = frequency; event.event_version = RSF_GAME_CPU_EVENT_VERSION;
            event.flags = RSF_GAME_CPU_INPUT_UNFRAMED;
        }
    }
    auto* previous_list = dispatched_list;
    dispatched_list = current_poll && current_poll->manager == manager ? list : nullptr;
    if (dispatched_list) current_poll->kinds |= event.input_kind;
    const auto result = reinterpret_cast<int (*)(void*, void*)>(hooks[3].original)(manager, list);
    dispatched_list = previous_list;
    // Worker polling is not assigned the latest tick. The runtime owns any bounded
    // association of this delivered input with an engine source and vendor policy.
    if (event.input_kind && host.cpu_event) {
        try { host.cpu_event(host.user, &event); } catch (...) {}
    }
    return result;
}
bool disable_hooks()
{
    bool disabled = true;
    for (auto& hook : hooks) if (hook.enabled) {
        const auto result = MH_DisableHook(hook.target);
        if (result == MH_OK || result == MH_ERROR_DISABLED) hook.enabled = false;
        else disabled = false;
    }
    return disabled;
}
short tick(void* main)
{
    Entry lease;
    const auto original = reinterpret_cast<short (*)(void*)>(hooks[0].original);
    if (!active.load(std::memory_order_acquire) || current) return original(main);
    Frame frame{}; frame.host = services; frame.main = main;
    // The previous generation's late poll becomes this tick's identity only when it was
    // sampled with exclusive control state and its native job generation joined normally.
    // Its actual input therefore predates this FRAME_BEGIN; binding still waits for the
    // normal-simulation submit. Anything else is withdrawn before this frame begins.
    frame.reservation = prospective.adopt(services.session_id, job_ownership.retired());
    if (frame.reservation.source) {
        frame.source = frame.reservation.source;
        // 0x743090 runs at 0x6588de, before the bind. It calls nothing unless player +0xBEC bit
        // 0x40 (test at 0x7430ed) or byte +0x692 (0x74314e) is set; its queued-move path reaches
        // camera update 0x7468b0 through 0x75ed50. Only 0xec1c40 and GetAsyncKeyState run between
        // this read and those tests on this thread.
        const auto base = uintptr_t(GetModuleHandleW(nullptr));
        uintptr_t player{}; uint8_t flags{}, pending{};
        frame.entry_readable = copy_input(&player, reinterpret_cast<const void*>(base + 0x3230778), sizeof(player)) &&
            player && copy_input(&flags, reinterpret_cast<const void*>(player + 0xbec), sizeof(flags)) &&
            copy_input(&pending, reinterpret_cast<const void*>(player + 0x692), sizeof(pending));
        frame.queued_move = (flags & 0x40) || pending;
    } else {
        if (const auto withdrawn = prospective.withdraw_pending())
            abort_source(frame.host, withdrawn, RSF_GAME_CPU_ABORT_STATE_CHANGED);
        frame.source = next_source.fetch_add(1);
    }
    current = &frame;
    emit(frame, RSF_GAME_CPU_FRAME_BEGIN);
    const auto result = original(main);
    if (frame.reservation.source) {
        // An adopted, unbound reservation (paused/frozen tick) closes unbound with FRAME_END.
        const auto owned = prospective.release(frame.source);
        if (owned.phase == ProspectiveSources::bound) worker_consumption.source.store(0);
        const auto& facts = owned.source ? owned : frame.reservation; // a refused bind released it early
        if (services.log && prospective_reports.fetch_add(1) < 16) {
            // PlayerCharacter update 0x745200 runs on this thread at 0x659f4d (via 0x659ea0, after
            // the submit) and drives PlayerCamera/camera-state updates; movement runs on workers.
            char message[560]{};
            std::snprintf(message, sizeof(message),
                "Skyrim prospective source=%llu predecessor=%llu poll_generation=%llu job_generation=%llu bound=%d refusal=%u kinds=0x%x main_player_update=%u main_camera=%u main_player_camera=%u main_movement=%u main_generation_match=%d main_ambiguous=%d worker_movement=%u worker_player_update=%u worker_camera=%u mismatched=%u; diagnostic facts, no input timeline",
                static_cast<unsigned long long>(frame.source), static_cast<unsigned long long>(facts.predecessor),
                static_cast<unsigned long long>(facts.poll_generation), static_cast<unsigned long long>(facts.job_generation),
                owned.phase == ProspectiveSources::bound, static_cast<unsigned>(frame.refusal), facts.kinds,
                frame.player_update_calls, frame.camera_updates, frame.player_camera_updates,
                frame.movement_calls, frame.consumed.generation == facts.poll_generation, frame.input_ambiguous,
                worker_consumption.movement.load(), worker_consumption.player_update.load(), worker_consumption.camera.load(),
                worker_consumption.mismatched.load());
            try { services.log(services.user, message); } catch (...) {}
        }
    }
    emit(frame, RSF_GAME_CPU_FRAME_END);
    current = nullptr;
    return result;
}
uintptr_t poll(void* manager, float timestep)
{
    Entry lease;
    // All native poll paths remain intact, including the independent mode-2 loop.
    PollScope scope{};
    auto* previous_poll = current_poll;
    scope.ticket = input_ownership.begin_poll();
    scope.manager = manager;
    current_poll = &scope;
    const bool running = active.load(std::memory_order_acquire);
    CpuPollProvenance provenance{};
    const bool scheduled = running && cpu_poll_provenance(provenance);
    if (running && poll_samples.fetch_add(1) < 16 && services.log) {
        char message[352]{};
        std::snprintf(message, sizeof(message),
            "Skyrim native poll thread=%lu scoped_source=%llu native_job_generation=%llu submitting_source=%llu poll_generation=%llu; submitting source is not input consumption",
            GetCurrentThreadId(), static_cast<unsigned long long>(current ? current->source : 0),
            static_cast<unsigned long long>(provenance.native_job_generation),
            static_cast<unsigned long long>(provenance.submitting_source_frame_id),
            static_cast<unsigned long long>(provenance.poll_generation));
        try { services.log(services.user, message); } catch (...) {}
    }
    uint64_t reserved{};
    if (running) {
        const auto host = services;
        if (scheduled) {
            // The verified mode1 late poll of the submitting source's generation. Its consumer
            // is a later tick; reserve that future identity before the unchanged native call.
            const auto source = next_source.fetch_add(1);
            const auto reservation = prospective.reserve(source, provenance.submitting_source_frame_id,
                provenance.session_id, provenance.native_job_generation, provenance.poll_generation);
            if (reservation.withdrawn) abort_source(host, reservation.withdrawn,
                reservation.overlap ? RSF_GAME_CPU_ABORT_OVERLAP : RSF_GAME_CPU_ABORT_INTERVENING_POLL);
            if (reservation.reserved.source) {
                reserved = source;
                emit_source(host, RSF_GAME_CPU_SOURCE_RESERVE, source, provenance.submitting_source_frame_id,
                    provenance.poll_generation, RSF_GAME_CPU_SOURCE_OVERLAPS_PREDECESSOR, 0, 0);
            }
        } else {
            // Mode2, paused main-thread, nested or unverified polls keep native behavior and
            // replace the controls any pending reservation sampled.
            if (const auto withdrawn = prospective.withdraw_pending())
                abort_source(host, withdrawn, RSF_GAME_CPU_ABORT_INTERVENING_POLL);
            if (worker_consumption.source.load()) worker_consumption.mismatched.fetch_add(1);
        }
    }
    // A scheduled late poll never samples the tick that happens to run it.
    if (current && !current->sampled && !scheduled) {
        current->sampled = true;
        emit(*current, RSF_GAME_CPU_INPUT_SAMPLE);
    }
    // The actual sample time: after the synchronous RESERVE callback (and anything the runtime
    // does inside it) has returned, immediately before the unchanged native poll.
    LARGE_INTEGER poll_begin{};
    if (reserved) QueryPerformanceCounter(&poll_begin);
    const auto result = reinterpret_cast<uintptr_t (*)(void*, float)>(hooks[1].original)(manager, timestep);
    current_poll = previous_poll;
    input_ownership.end_poll(scope.ticket, scope.produced && !previous_poll);
    if (reserved) {
        // Produced only when this exact poll left the completed, unchanged control state.
        const auto host = services;
        const bool produced = input_ownership.snapshot().generation == scope.ticket.generation;
        if (!produced) {
            if (prospective.withdraw_pending(reserved)) abort_source(host, reserved, RSF_GAME_CPU_ABORT_NOT_PRODUCED);
        } else if (prospective.still_reserved(reserved)) {
            // Report before the adapter can adopt it, so the runtime never sees the frame first.
            emit_source(host, RSF_GAME_CPU_SOURCE_SAMPLED, reserved, 0, scope.ticket.generation,
                RSF_GAME_CPU_SOURCE_PRODUCED, scope.kinds, 0, uint64_t(poll_begin.QuadPart));
            if (!prospective.sample(reserved, true, scope.kinds) && prospective.withdraw_pending(reserved))
                abort_source(host, reserved, RSF_GAME_CPU_ABORT_STATE_CHANGED);
        }
    }
    return result;
}
uintptr_t controls(void* owner, void* list, void* source)
{
    Entry lease;
    // BSTEventSink also receives its event-source pointer in R8. Forward it even
    // though this implementation does not read it in the matched executable.
    input_ownership.begin_write();
    if (!current_poll || !list || list != dispatched_list) input_ownership.invalidate();
    const auto result = reinterpret_cast<uintptr_t (*)(void*, void*, void*)>(hooks[5].original)(owner, list, source);
    uintptr_t singleton{};
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    if (active.load(std::memory_order_acquire) && current_poll && list && list == dispatched_list &&
        copy_input(&singleton, reinterpret_cast<const void*>(base + 0x31a5698), sizeof(singleton)) &&
        singleton == uintptr_t(owner)) current_poll->produced = true;
    input_ownership.end_write();
    return result;
}
void record_consumption(InputOwnership::Stamp stamp, uint32_t& count)
{
    ++count;
    if (!input_ownership.unchanged(stamp) ||
        (current->consumed.generation && current->consumed.generation != stamp.generation)) {
        current->input_ambiguous = true;
    } else current->consumed = stamp;
}
// Off the main tick: count only inside the bound source's own published job generation.
// A different completed poll generation is counted as a mismatch, never as consumption.
void record_worker_consumption(InputOwnership::Stamp stamp, std::atomic<uint32_t>& count)
{
    const auto source = worker_consumption.source.load();
    if (!source) return;
    const auto ticket = job_ownership.snapshot();
    const auto owned = prospective.owned();
    if (!ticket.generation || ticket.source != source || owned.source != source ||
        owned.phase != ProspectiveSources::bound) return;
    if (stamp.generation == owned.poll_generation && input_ownership.unchanged(stamp)) count.fetch_add(1);
    else worker_consumption.mismatched.fetch_add(1);
}
bool is_player(void* actor) noexcept
{
    uintptr_t singleton{};
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    return copy_input(&singleton, reinterpret_cast<const void*>(base + 0x3230778), sizeof(singleton)) &&
        singleton && singleton == uintptr_t(actor);
}
void movement(void* actor, float timestep, void* argument, float* output)
{
    Entry lease;
    const auto stamp = input_ownership.snapshot();
    const bool player = is_player(actor);
    reinterpret_cast<void (*)(void*, float, void*, float*)>(hooks[10].original)(actor, timestep, argument, output);
    if (active.load(std::memory_order_acquire) && player && is_player(actor)) {
        if (current) record_consumption(stamp, current->movement_calls);
        else record_worker_consumption(stamp, worker_consumption.movement);
    }
}
uintptr_t player_update(void* actor, float timestep)
{
    Entry lease;
    const auto stamp = input_ownership.snapshot();
    const bool player = is_player(actor);
    const auto result = reinterpret_cast<uintptr_t (*)(void*, float)>(hooks[11].original)(actor, timestep);
    if (active.load(std::memory_order_acquire) && player && is_player(actor)) {
        if (current) record_consumption(stamp, current->player_update_calls);
        else record_worker_consumption(stamp, worker_consumption.player_update);
    }
    return result;
}
uintptr_t camera_update(void* state)
{
    Entry lease;
    const auto stamp = input_ownership.snapshot();
    uint8_t disabled{1};
    const bool consumes = copy_input(&disabled, static_cast<const unsigned char*>(state) + 0xe0, sizeof(disabled)) && !disabled;
    const auto result = reinterpret_cast<uintptr_t (*)(void*)>(hooks[6].original)(state);
    if (active.load(std::memory_order_acquire) && current) {
        if (!consumes) current->input_ambiguous = true;
        record_consumption(stamp, current->camera_updates);
    } else if (active.load(std::memory_order_acquire) && consumes) record_worker_consumption(stamp, worker_consumption.camera);
    return result;
}
// Every PlayerCamera state update (first person, third person, furniture, free, load and
// queued-move paths) runs through this TESCamera::Update with the PlayerCamera singleton.
void player_camera_update(void* camera)
{
    Entry lease;
    const auto stamp = input_ownership.snapshot();
    uintptr_t singleton{};
    const bool player_camera = copy_input(&singleton,
        reinterpret_cast<const void*>(uintptr_t(GetModuleHandleW(nullptr)) + 0x31a5478), sizeof(singleton)) &&
        singleton && singleton == uintptr_t(camera);
    reinterpret_cast<void (*)(void*)>(hooks[12].original)(camera);
    if (active.load(std::memory_order_acquire) && current && player_camera)
        record_consumption(stamp, current->player_camera_updates);
}
uintptr_t render(void* main)
{
    Entry lease;
    const auto previous_scope = current ? current->render_scope : 0;
    if (current) {
        current->screen = native_screen(main, false);
        current->render_scope = next_render_scope.fetch_add(1);
        current->render_entered = true;
        if (!current->simulation_ended) {
            current->simulation_ended = true;
            emit(*current, RSF_GAME_CPU_SIMULATION_END);
        }
    }
    auto pass = render_fact(RSF_GAME_RENDER_FRAME);
    report_render(pass, 1);
    const auto result = reinterpret_cast<uintptr_t (*)(void*)>(hooks[2].original)(main);
    if (current) (void)cpu_render_identity(pass);
    report_render(pass, 0);
    if (current) current->render_scope = previous_scope;
    return result;
}
}

bool cpu_prepare(const rsf_game_host_services& host)
{
    if (active.load() || entered.load()) return false;
    auto* base = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr));
    uintptr_t named_callback{};
    if (!copy_input(&named_callback, base + 0x20b5130, sizeof(named_callback)) ||
        named_callback != uintptr_t(base + 0x6e6030)) return false;
    for (const auto& query : screen_query_guards)
        if (std::memcmp(base + query.rva, query.bytes.data(), query.bytes.size())) return false;
    for (auto& hook : hooks) {
        hook.target = base + hook.rva;
        if (std::memcmp(hook.target, hook.bytes.data(), hook.bytes.size())) return false;
    }
    for (auto& hook : hooks) {
        if (hook.created) continue;
        if (MH_CreateHook(hook.target, hook.detour, &hook.original) != MH_OK) {
            for (auto& previous : hooks) if (previous.created) {
                MH_RemoveHook(previous.target); previous.created = false; previous.original = nullptr;
            }
            return false;
        }
        hook.created = true;
    }
    LARGE_INTEGER value{};
    if (!QueryPerformanceFrequency(&value) || value.QuadPart <= 0) return false;
    frequency = uint64_t(value.QuadPart); services = host;
    return true;
}
bool cpu_start()
{
    if (active.load(std::memory_order_acquire) || entered.load(std::memory_order_acquire)) return false;
    job_ownership.reset();
    input_ownership.invalidate();
    prospective.reset();
    worker_consumption.source.store(0);
    for (auto& hook : hooks) {
        if (!hook.created || MH_EnableHook(hook.target) != MH_OK) {
            // A failed start leaves preparation and its borrowed services valid for retry.
            (void)disable_hooks(); return false;
        }
        hook.enabled = true;
    }
    active.store(true, std::memory_order_release); return true;
}
void cpu_quiesce() {
    active.store(false, std::memory_order_release);
    job_ownership.invalidate();
    input_ownership.invalidate();
    // No events here: the runtime closes every reservation when it releases the session.
    prospective.reset();
    worker_consumption.source.store(0);
}
bool cpu_stop()
{
    cpu_quiesce();
    const bool disabled = disable_hooks();
    if (!disabled || entered.load(std::memory_order_acquire)) return false;
    services = {}; return true;
}
bool cpu_busy() { return entered.load(std::memory_order_acquire) != 0; }
uint64_t cpu_source() { return current ? current->source : 0; }
bool cpu_render_identity(rsf_game_render_pass& pass) {
    if (!current || !current->render_scope || !current->simulation_ended) return false;
    pass.source_frame_id = current->source;
    pass.submission_id = pass.scope_id = current->render_scope;
    pass.screen = current->screen;
    pass.flags |= RSF_GAME_RENDER_AFTER_SIMULATION | RSF_GAME_RENDER_NO_FG;
    if (cpu_rendered_source_eligible()) pass.flags &= ~RSF_GAME_RENDER_NO_FG;
    return true;
}
bool cpu_rendered_source_eligible() noexcept {
    return current && rendered_source_eligible(active.load(std::memory_order_acquire) && current->render_scope,
        current->primary_world_entries, current->screen);
}
uint32_t cpu_input_kinds(const void* list) noexcept { return input_kinds(list); }
bool cpu_input_consumption(CpuInputConsumption& result) noexcept {
    result = {};
    if (!active.load(std::memory_order_acquire) || !current || current->input_ambiguous ||
        !current->camera_updates || !input_ownership.unchanged(current->consumed)) return false;
    result.source_frame_id = current->source;
    result.poll_generation = current->consumed.generation;
    result.camera_updates = current->camera_updates;
    result.movement_calls = current->movement_calls;
    result.player_update_calls = current->player_update_calls;
    return true;
}
bool cpu_poll_provenance(CpuPollProvenance& result) noexcept {
    result = {};
    ManagerState native{};
    uintptr_t input_manager{};
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    if (!active.load(std::memory_order_acquire) || !current_poll || !input_ownership.owns_poll(current_poll->ticket) ||
        !job_ownership.matches(named_poll_jobs) || !native.read() || native.manager != named_poll_jobs.manager ||
        native.requested > 1 || native.running != 1 ||
        !copy_input(&input_manager, reinterpret_cast<const void*>(base + 0x3204b30), sizeof(input_manager)) ||
        !input_manager || input_manager != uintptr_t(current_poll->manager) ||
        !job_ownership.matches(named_poll_jobs) || !input_ownership.owns_poll(current_poll->ticket) ||
        !active.load(std::memory_order_acquire)) return false;
    result.session_id = named_poll_jobs.session;
    result.native_job_generation = named_poll_jobs.generation;
    result.submitting_source_frame_id = named_poll_jobs.source;
    result.poll_generation = current_poll->ticket.generation;
    return true;
}
}
