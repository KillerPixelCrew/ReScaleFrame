// SPDX-License-Identifier: MIT
#include "cpu_events.h"
#include "cpu_policy.h"
#include "input_ownership.h"
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
extern "C" {
void* rsf_skyrim_sources_new() noexcept;
void rsf_skyrim_sources_free(void*) noexcept;
void rsf_skyrim_sources_reserve(const void*, ProspectiveSources::Reserve*, uint64_t source,
    uint64_t predecessor, uint64_t session, uint64_t job_generation, uint64_t poll_generation) noexcept;
bool rsf_skyrim_sources_still_reserved(const void*, uint64_t) noexcept;
bool rsf_skyrim_sources_sample(const void*, uint64_t, bool, uint32_t) noexcept;
uint64_t rsf_skyrim_sources_withdraw_pending(const void*, uint64_t) noexcept;
void rsf_skyrim_sources_adopt(const void*, ProspectiveSources::Reservation*, uint64_t, uint64_t) noexcept;
bool rsf_skyrim_sources_bind(const void*, uint64_t, uint64_t) noexcept;
void rsf_skyrim_sources_owned(const void*, ProspectiveSources::Reservation*) noexcept;
void rsf_skyrim_sources_release(const void*, ProspectiveSources::Reservation*, uint64_t) noexcept;
void rsf_skyrim_sources_reset(const void*) noexcept;
uint32_t rsf_skyrim_bind_refusal(bool, bool, bool, bool) noexcept;
}

ProspectiveSources::ProspectiveSources() noexcept : store_(rsf_skyrim_sources_new()) {}
ProspectiveSources::~ProspectiveSources() { if (store_) rsf_skyrim_sources_free(store_); }
ProspectiveSources::Reserve ProspectiveSources::reserve(uint64_t source, uint64_t predecessor, uint64_t session,
    uint64_t job_generation, uint64_t poll_generation) noexcept {
    Reserve out{};
    rsf_skyrim_sources_reserve(store_, &out, source, predecessor, session, job_generation, poll_generation);
    return out;
}
bool ProspectiveSources::still_reserved(uint64_t source) const noexcept {
    return rsf_skyrim_sources_still_reserved(store_, source);
}
bool ProspectiveSources::sample(uint64_t source, bool produced, uint32_t kinds) noexcept {
    return rsf_skyrim_sources_sample(store_, source, produced, kinds);
}
uint64_t ProspectiveSources::withdraw_pending(uint64_t source) noexcept {
    return rsf_skyrim_sources_withdraw_pending(store_, source);
}
ProspectiveSources::Reservation ProspectiveSources::adopt(uint64_t session, uint64_t joined) noexcept {
    Reservation out{};
    rsf_skyrim_sources_adopt(store_, &out, session, joined);
    return out;
}
bool ProspectiveSources::bind(uint64_t source, uint64_t completed) noexcept {
    return rsf_skyrim_sources_bind(store_, source, completed);
}
ProspectiveSources::Reservation ProspectiveSources::owned() const noexcept {
    Reservation out{};
    rsf_skyrim_sources_owned(store_, &out);
    return out;
}
ProspectiveSources::Reservation ProspectiveSources::release(uint64_t source) noexcept {
    Reservation out{};
    rsf_skyrim_sources_release(store_, &out, source);
    return out;
}
void ProspectiveSources::reset() noexcept { rsf_skyrim_sources_reset(store_); }
BindRefusal bind_refusal(bool readable, bool queued_move, bool consumed_before_bind, bool free_camera) noexcept {
    return static_cast<BindRefusal>(rsf_skyrim_bind_refusal(readable, queued_move, consumed_before_bind, free_camera));
}

namespace {
struct Frame {
    rsf_game_host_services host{};
    uint64_t source{};
    rsf_skyrim_cpu_render_state render{};
    InputOwnership::Stamp consumed{};
    uint32_t camera_updates{};
    bool input_ambiguous{};
    NativeJobOwnership::Ticket jobs{};
    void* main{};
    uint32_t movement_calls{}, player_update_calls{};
    uint32_t player_camera_updates{}; // TESCamera::Update 0x558e70 on PlayerCamera, any state
    ProspectiveSources::Reservation reservation{}; // adopted at tick entry, if any
    bool entry_readable{}, queued_move{}; // player queued-move gates read at tick entry
    BindRefusal refusal{BindRefusal::none};
};
thread_local Frame* current{};
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
ProspectiveSources prospective{ProspectiveSources::Storage::process};
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
    rsf_skyrim_cpu_source_event(&event, stage, host.session_id, source, uint64_t(now.QuadPart), frequency,
        predecessor, generation, flags, kinds, reason, poll_begin);
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
    rsf_skyrim_cpu_frame_event(&event, &frame.render, stage, frame.host.session_id, frame.source,
        uint64_t(now.QuadPart), frequency);
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
            // Bind the adopted poll here only while its completed control state is unchanged and
            // no consumer ran earlier in the tick: the pre-submit callees read no controls
            // (prospective-source note), 0x743090 is refused through its queued-move gates, and
            // hooked player/movement/camera consumers before this point refuse the bind. Later
            // consumers are observed by the stamp counters, not assumed.
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
                        abort_source(current->host, current->source, RSF_GAME_CPU_ABORT_NOT_CONSUMED);
                    }
                } else if (prospective.bind(current->source, stamp.generation)) {
                    worker_consumption.source.store(0);
                    worker_consumption.movement.store(0); worker_consumption.player_update.store(0);
                    worker_consumption.camera.store(0); worker_consumption.mismatched.store(0);
                    worker_consumption.source.store(current->source);
                    emit_source(current->host, RSF_GAME_CPU_SOURCE_BIND, current->source, 0, 0, 0, 0, 0);
                } else if (prospective.release(current->source).source) {
                    abort_source(current->host, current->source, RSF_GAME_CPU_ABORT_STATE_CHANGED);
                }
            }
            // This verified normal-simulation branch starts actor/player work even when
            // prospective input ownership was refused. Report the engine fact independently.
            emit(*current, RSF_GAME_CPU_SIMULATION_BEGIN);
            // Original wakes the manager only after setting both native mode1 fields.
            // Publish immutable scalars first so a newly awakened worker sees them.
            ticket = job_ownership.publish(before.manager, current->source, current->host.session_id, true);
            current->jobs = ticket;
        } else if (current || job_ownership.snapshot().generation) {
            // Mid-tick activation has no owned source. Only a real outstanding ticket
            // or a refused owned submit invalidates provenance.
            job_ownership.invalidate();
        }
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
bool query_menus(uintptr_t open_address, uintptr_t frozen_address, uintptr_t ui, uintptr_t loading_name,
    uintptr_t main_name, NativeScreenFacts* facts) noexcept {
    const auto open = reinterpret_cast<bool (*)(void*, const void*)>(open_address);
    const auto frozen = reinterpret_cast<bool (*)(void*)>(frozen_address);
    auto* owner = reinterpret_cast<void*>(ui);
    try {
        facts->loading_menu = open(owner, reinterpret_cast<const void*>(loading_name));
        facts->main_menu = open(owner, reinterpret_cast<const void*>(main_name));
        facts->frozen_background = frozen(owner);
        return true;
    } catch (...) { return false; }
}
rsf_game_render_pass render_fact(uint32_t role) {
    rsf_game_render_pass pass{}; pass.struct_size = sizeof(pass); pass.role = role;
    if (!current || !current->host.render_pass || !cpu_render_identity(pass)) return pass;
    rsf_skyrim_cpu_render_fact(&pass, current->host.session_id);
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
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    const bool primary = current && rsf_skyrim_cpu_begin_world(&current->render, &pass, base, caller, output);
    if (primary) {
        const auto screen = rsf_skyrim_cpu_screen(uintptr_t(main), true, query_menus);
        rsf_skyrim_cpu_world_screen(&current->render, screen);
        (void)cpu_render_identity(pass);
        report_render(pass, 1);
    }
    const auto result = reinterpret_cast<uintptr_t (*)(void*, uint32_t, uint8_t)>(hooks[4].original)(main, output, flags);
    if (primary) {
        rsf_skyrim_cpu_end_world(&current->render);
        (void)cpu_render_identity(pass);
        report_render(pass, 0);
    }
    return result;
}
int dispatch(void* manager, void* list) {
    Entry lease;
    rsf_game_cpu_event event{};
    rsf_game_host_services host{};
    const auto base = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr));
    uintptr_t native_manager{};
    if (active.load(std::memory_order_acquire) && copy_input(&native_manager, base + 0x3204b30, sizeof(native_manager)) &&
        uintptr_t(manager) == native_manager) {
        if (const auto kinds = cpu_input_kinds(list)) {
            host = services;
            LARGE_INTEGER now{}; QueryPerformanceCounter(&now);
            rsf_skyrim_cpu_input_event(&event, host.session_id, kinds, uint64_t(now.QuadPart), frequency);
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
    const auto original = reinterpret_cast<short (*)(void*)>(hooks[0].original);
    if (!active.load(std::memory_order_acquire) || current) return original(main);
    Entry lease;
    if (!active.load(std::memory_order_acquire)) return original(main);
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
        frame.source = rsf_skyrim_cpu_next_source();
    }
    current = &frame;
    emit(frame, RSF_GAME_CPU_FRAME_BEGIN);
    const auto result = original(main);
    if (frame.reservation.source) {
        // An adopted, unbound reservation (paused/frozen tick) closes unbound with FRAME_END.
        const auto owned = prospective.release(frame.source);
        if (owned.phase == ProspectiveSources::bound) worker_consumption.source.store(0);
        const auto& facts = frame.reservation; // immutable adoption facts, including a refused bind
        if (services.log && prospective_reports.fetch_add(1) < 16) {
            // PlayerCharacter update 0x745200 runs on this thread at 0x659f4d (via 0x659ea0, after
            // the submit) and drives PlayerCamera/camera-state updates; movement runs on workers.
            char message[560]{};
            std::snprintf(message, sizeof(message),
                "Skyrim prospective source=%llu predecessor=%llu poll_generation=%llu job_generation=%llu bound=%d refusal=%u kinds=0x%x main_player_update=%u main_camera=%u main_player_camera=%u main_movement=%u main_generation_match=%d main_ambiguous=%d worker_movement=%u worker_player_update=%u worker_camera=%u mismatched=%u; input timeline ownership diagnostics",
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
            const auto source = rsf_skyrim_cpu_next_source();
            const auto reservation = prospective.reserve(source, provenance.submitting_source_frame_id,
                provenance.session_id, provenance.native_job_generation, provenance.poll_generation);
            if (reservation.withdrawn) abort_source(host, reservation.withdrawn,
                reservation.overlap ? RSF_GAME_CPU_ABORT_OVERLAP : RSF_GAME_CPU_ABORT_INTERVENING_POLL);
            if (reservation.reserved.source) {
                reserved = source;
                emit_source(host, RSF_GAME_CPU_SOURCE_RESERVE, source, provenance.submitting_source_frame_id,
                    provenance.poll_generation, RSF_GAME_CPU_SOURCE_OVERLAPS_PREDECESSOR, 0, 0);
                // Withdrawal can win between local reservation and the host callback.
                // Repeat its terminal fact after RESERVE so the host cannot retain it open.
                if (!prospective.still_reserved(source)) {
                    abort_source(host, source, RSF_GAME_CPU_ABORT_STATE_CHANGED);
                    reserved = 0;
                }
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
    if (current && !scheduled && rsf_skyrim_cpu_sample_tick(&current->render)) {
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
    const auto previous_scope = current ? current->render.scope : 0;
    if (current) {
        const auto screen = rsf_skyrim_cpu_screen(uintptr_t(main), false, query_menus);
        if (rsf_skyrim_cpu_begin_render(&current->render, screen)) {
            emit(*current, RSF_GAME_CPU_SIMULATION_END);
        }
    }
    auto pass = render_fact(RSF_GAME_RENDER_FRAME);
    report_render(pass, 1);
    const auto result = reinterpret_cast<uintptr_t (*)(void*)>(hooks[2].original)(main);
    if (current) (void)cpu_render_identity(pass);
    report_render(pass, 0);
    if (current) rsf_skyrim_cpu_end_render(&current->render, previous_scope);
    return result;
}
}

bool cpu_prepare(const rsf_game_host_services& host, const rsf_skyrim_creation_platform* platform)
{
    if (active.load() || entered.load()) return false;
    auto* base = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr));
    uintptr_t named_callback{};
    if (!copy_input(&named_callback, base + 0x20b5130, sizeof(named_callback)) ||
        named_callback != uintptr_t(base + 0x6e6030)) return false;
    if (rsf_skyrim_cpu_bad_screen_query(platform, uintptr_t(base))) return false;
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
bool cpu_start(const rsf_skyrim_creation_platform* platform)
{
    const auto refused = [](const char* stage, uint32_t rva, int status = 0) {
        if (services.log) {
            char message[180]{};
            std::snprintf(message, sizeof(message), "Skyrim CPU start refused: %s RVA=%X status=%d", stage, rva, status);
            services.log(services.user, message);
        }
        return false;
    };
    if (active.load(std::memory_order_acquire) || entered.load(std::memory_order_acquire)) return refused("active or entered", 0);
    // Only disabled targets contain the original guard bytes. Never compare our
    // own enabled MinHook jump against the original prologue on a failed rollback.
    for (const auto& hook : hooks) if (!hook.created || hook.enabled) return refused("hook state", hook.rva);
    const auto* base = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr));
    uintptr_t named_callback{};
    if (!copy_input(&named_callback, base + 0x20b5130, sizeof(named_callback)) ||
        named_callback != uintptr_t(base + 0x6e6030)) return refused("named callback", 0x20b5130);
    if (const auto rva = rsf_skyrim_cpu_bad_screen_query(platform, uintptr_t(base)))
        return refused("screen query bytes", rva);
    for (const auto& hook : hooks)
        if (std::memcmp(hook.target, hook.bytes.data(), hook.bytes.size())) return refused("hook bytes", hook.rva);
    job_ownership.reset();
    input_ownership.invalidate();
    prospective.reset();
    worker_consumption.source.store(0);
    for (auto& hook : hooks) {
        const auto status = hook.created ? MH_EnableHook(hook.target) : MH_ERROR_NOT_CREATED;
        if (status != MH_OK) {
            // A failed start leaves preparation and its borrowed services valid for retry.
            (void)disable_hooks(); return refused("hook enable", hook.rva, int(status));
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
bool cpu_disable()
{
    cpu_quiesce();
    const bool disabled = disable_hooks();
    if (!disabled || entered.load(std::memory_order_acquire)) return false;
    return true;
}
bool cpu_stop()
{
    if (!cpu_disable()) return false;
    services = {}; return true;
}
bool cpu_busy() { return entered.load(std::memory_order_acquire) != 0; }
uint64_t cpu_source() { return current ? current->source : 0; }
bool cpu_render_identity(rsf_game_render_pass& pass) {
    return current && rsf_skyrim_cpu_render_identity(&current->render, &pass, current->source,
        active.load(std::memory_order_acquire));
}
bool cpu_rendered_source_eligible() noexcept {
    return current && rendered_source_eligible(active.load(std::memory_order_acquire) && current->render.scope,
        current->render.primary_world_entries, current->render.screen);
}
void cpu_render_diagnostic(const char* stage, const rsf_game_render_pass& pass) {
    if (!current || !current->host.log || (current->source > 8 && current->source % 128 != 0)) return;
    static std::atomic<uint32_t> reports{};
    if (reports.fetch_add(1, std::memory_order_relaxed) >= 64) return;
    const auto& frame = *current;
    const auto owned = prospective.owned();
    char message[760]{};
    std::snprintf(message, sizeof(message),
        "Skyrim render producer stage=%s source=%llu cpu_source=%llu native=%llu scope=%llu cpu_scope=%llu flags=0x%x screen=%u cpu_screen=%u active=%u world_entries=%u eligible=%u render_entered=%u simulation_ended=%u primary_count=%u reservation=%llu owned_source=%llu owned_phase=%u bound=%u refusal=%u poll=%llu job=%llu entry_readable=%u queued_move=%u input_ambiguous=%u; diagnostic only",
        stage, static_cast<unsigned long long>(pass.source_frame_id), static_cast<unsigned long long>(frame.source),
        static_cast<unsigned long long>(pass.native_frame), static_cast<unsigned long long>(pass.scope_id),
        static_cast<unsigned long long>(frame.render.scope), pass.flags, unsigned(pass.screen), unsigned(frame.render.screen),
        unsigned(active.load(std::memory_order_acquire)), frame.render.primary_world_entries, unsigned(cpu_rendered_source_eligible()),
        unsigned(frame.render.render_entered), unsigned(frame.render.simulation_ended), frame.render.primary_count,
        static_cast<unsigned long long>(frame.reservation.source), static_cast<unsigned long long>(owned.source), unsigned(owned.phase),
        unsigned(owned.source == frame.source && owned.phase == ProspectiveSources::bound), unsigned(frame.refusal),
        static_cast<unsigned long long>(frame.reservation.poll_generation), static_cast<unsigned long long>(frame.reservation.job_generation),
        unsigned(frame.entry_readable), unsigned(frame.queued_move), unsigned(frame.input_ambiguous));
    try { frame.host.log(frame.host.user, message); } catch (...) {}
}
uint32_t cpu_input_kinds(const void* list) noexcept {
    return rsf_skyrim_cpu_input_kinds(uintptr_t(list), [](uintptr_t address, void* out, size_t size) noexcept {
        return copy_input(out, reinterpret_cast<const void*>(address), size);
    });
}
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
