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
namespace {
// Private Rust lifecycle boundary, mirrored in cpu_hooks.rs. The native wrappers below
// preserve the fixture and session subsystem interfaces.
struct CpuHookLeaves {
    uint32_t struct_size, version;
    bool (*create)(uint32_t index, uintptr_t target);
    bool (*remove)(uint32_t index, uintptr_t target);
    int32_t (*enable)(uintptr_t target);
    bool (*disable)(uintptr_t target);
    uint64_t (*qpc_frequency)();
};
struct CpuServices {
    rsf_game_host_services host{};
    uint64_t frequency{};
};
struct CpuStartRefusal {
    const char* stage;
    uint32_t rva;
    int32_t status;
};
static_assert(sizeof(CpuHookLeaves) == 48);
static_assert(sizeof(CpuServices) == sizeof(rsf_game_host_services) + 8 &&
    offsetof(CpuServices, frequency) == sizeof(rsf_game_host_services));
static_assert(sizeof(CpuStartRefusal) == 16);
}
extern "C" {
bool rsf_skyrim_cpu_rust_bind(const rsf_game_host_services*) noexcept;
bool rsf_skyrim_cpu_rust_prepare(const CpuHookLeaves*) noexcept;
bool rsf_skyrim_cpu_rust_start(const CpuHookLeaves*, CpuStartRefusal*) noexcept;
void rsf_skyrim_cpu_rust_quiesce() noexcept;
bool rsf_skyrim_cpu_rust_disable(const CpuHookLeaves*) noexcept;
bool rsf_skyrim_cpu_rust_stop(const CpuHookLeaves*) noexcept;
bool rsf_skyrim_cpu_rust_busy() noexcept;
void rsf_skyrim_cpu_services(CpuServices*) noexcept;
bool rsf_skyrim_cpu_active() noexcept;
void rsf_skyrim_cpu_enter() noexcept;
void rsf_skyrim_cpu_leave() noexcept;
void* rsf_skyrim_jobs_new() noexcept;
void rsf_skyrim_jobs_free(void*) noexcept;
void rsf_skyrim_jobs_publish(const void*, NativeJobOwnership::Ticket*, uintptr_t, uint64_t, uint64_t, bool) noexcept;
void rsf_skyrim_jobs_snapshot(const void*, NativeJobOwnership::Ticket*) noexcept;
bool rsf_skyrim_jobs_matches(const void*, const NativeJobOwnership::Ticket*) noexcept;
bool rsf_skyrim_jobs_retire(const void*, const NativeJobOwnership::Ticket*) noexcept;
uint64_t rsf_skyrim_jobs_retired(const void*) noexcept;
void rsf_skyrim_jobs_invalidate(const void*) noexcept;
void rsf_skyrim_jobs_reset(const void*) noexcept;
void* rsf_skyrim_epochs_new() noexcept;
void rsf_skyrim_epochs_free(void*) noexcept;
void rsf_skyrim_epochs_begin_poll(const void*, InputOwnership::Poll*) noexcept;
void rsf_skyrim_epochs_end_poll(const void*, const InputOwnership::Poll*, bool) noexcept;
bool rsf_skyrim_epochs_owns_poll(const void*, const InputOwnership::Poll*) noexcept;
void rsf_skyrim_epochs_snapshot(const void*, InputOwnership::Stamp*) noexcept;
bool rsf_skyrim_epochs_unchanged(const void*, const InputOwnership::Stamp*) noexcept;
void rsf_skyrim_epochs_begin_write(const void*) noexcept;
void rsf_skyrim_epochs_end_write(const void*) noexcept;
void rsf_skyrim_epochs_invalidate(const void*) noexcept;
void rsf_skyrim_poll_begin(PollScope*, uintptr_t manager) noexcept;
void rsf_skyrim_poll_end(const PollScope*, bool nested) noexcept;
void rsf_skyrim_poll_record_kinds(PollScope*, uint32_t kinds) noexcept;
void rsf_skyrim_poll_sink_produced(PollScope*) noexcept;
bool rsf_skyrim_poll_provenance(CpuPollProvenance*, const PollScope*, const NativeJobOwnership::Ticket*,
    uintptr_t base, bool (*read_managers)(uintptr_t, uintptr_t, uintptr_t)) noexcept;
PollSample rsf_skyrim_poll_sample_status(const PollScope*, uint64_t source) noexcept;
void rsf_skyrim_worker_bind(uint64_t source) noexcept;
void rsf_skyrim_worker_release() noexcept;
void rsf_skyrim_worker_intervening_poll() noexcept;
void rsf_skyrim_worker_record_consumer(const InputOwnership::Stamp*, CpuConsumer) noexcept;
void rsf_skyrim_worker_counts(WorkerCounts*) noexcept;
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

static_assert(sizeof(CpuPollProvenance) == 32 && offsetof(CpuPollProvenance, poll_generation) == 24);

NativeJobOwnership::NativeJobOwnership() noexcept : store_(rsf_skyrim_jobs_new()) {}
NativeJobOwnership::~NativeJobOwnership() { if (store_) rsf_skyrim_jobs_free(store_); }
NativeJobOwnership::Ticket NativeJobOwnership::publish(
    uintptr_t manager, uint64_t source, uint64_t session, bool native_idle) noexcept {
    Ticket out{};
    rsf_skyrim_jobs_publish(store_, &out, manager, source, session, native_idle);
    return out;
}
NativeJobOwnership::Ticket NativeJobOwnership::snapshot() const noexcept {
    Ticket out{};
    rsf_skyrim_jobs_snapshot(store_, &out);
    return out;
}
bool NativeJobOwnership::matches(const Ticket& ticket) const noexcept {
    return rsf_skyrim_jobs_matches(store_, &ticket);
}
bool NativeJobOwnership::retire(const Ticket& ticket) noexcept { return rsf_skyrim_jobs_retire(store_, &ticket); }
uint64_t NativeJobOwnership::retired() const noexcept { return rsf_skyrim_jobs_retired(store_); }
void NativeJobOwnership::invalidate() noexcept { rsf_skyrim_jobs_invalidate(store_); }
void NativeJobOwnership::reset() noexcept { rsf_skyrim_jobs_reset(store_); }

InputOwnership::InputOwnership() noexcept : store_(rsf_skyrim_epochs_new()) {}
InputOwnership::~InputOwnership() { if (store_) rsf_skyrim_epochs_free(store_); }
InputOwnership::Poll InputOwnership::begin_poll() noexcept {
    Poll out{};
    rsf_skyrim_epochs_begin_poll(store_, &out);
    return out;
}
void InputOwnership::end_poll(const Poll& poll, bool produced) noexcept {
    rsf_skyrim_epochs_end_poll(store_, &poll, produced);
}
bool InputOwnership::owns_poll(const Poll& poll) const noexcept { return rsf_skyrim_epochs_owns_poll(store_, &poll); }
InputOwnership::Stamp InputOwnership::snapshot() const noexcept {
    Stamp out{};
    rsf_skyrim_epochs_snapshot(store_, &out);
    return out;
}
bool InputOwnership::unchanged(const Stamp& stamp) const noexcept { return rsf_skyrim_epochs_unchanged(store_, &stamp); }
void InputOwnership::begin_write() noexcept { rsf_skyrim_epochs_begin_write(store_); }
void InputOwnership::end_write() noexcept { rsf_skyrim_epochs_end_write(store_); }
void InputOwnership::invalidate() noexcept { rsf_skyrim_epochs_invalidate(store_); }

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
    CpuServices services{};
    uint64_t source{};
    rsf_skyrim_cpu_render_state render{};
    rsf_skyrim_cpu_frame_input input{};
    NativeJobOwnership::Ticket jobs{};
    void* main{};
    ProspectiveSources::Reservation reservation{}; // adopted at tick entry, if any
    bool entry_readable{}, queued_move{}; // player queued-move gates read at tick entry
    BindRefusal refusal{BindRefusal::none};
};
thread_local Frame* current{};
std::atomic<uint32_t> poll_samples{};
InputOwnership input_ownership{CpuProofStorage::process};
NativeJobOwnership job_ownership{CpuProofStorage::process};
thread_local NativeJobOwnership::Ticket named_poll_jobs{};
thread_local PollScope* current_poll{};
thread_local void* dispatched_list{};
ProspectiveSources prospective{ProspectiveSources::Storage::process};
std::atomic<uint32_t> prospective_reports{};
struct Entry {
    Entry() noexcept { rsf_skyrim_cpu_enter(); }
    ~Entry() { rsf_skyrim_cpu_leave(); }
};
CpuServices cpu_services() noexcept {
    CpuServices result{};
    rsf_skyrim_cpu_services(&result);
    return result;
}
// Version 2 source lifetime fact. The runtime decides what, if anything, it means.
void emit_source(const CpuServices& services, uint32_t stage, uint64_t source, uint64_t predecessor,
    uint64_t generation, uint32_t flags, uint32_t kinds, uint32_t reason, uint64_t poll_begin = 0)
{
    const auto& host = services.host;
    if (!host.cpu_event || !source) return;
    LARGE_INTEGER now{}; QueryPerformanceCounter(&now);
    rsf_game_cpu_source_event event{};
    rsf_skyrim_cpu_source_event(&event, stage, host.session_id, source, uint64_t(now.QuadPart), services.frequency,
        predecessor, generation, flags, kinds, reason, poll_begin);
    try { host.cpu_event(host.user, &event.header); } catch (...) {}
}
void abort_source(const CpuServices& services, uint64_t source, uint32_t reason)
{
    emit_source(services, RSF_GAME_CPU_SOURCE_ABORT, source, 0, 0, 0, 0, reason);
}
void emit(Frame& frame, uint32_t stage)
{
    if (!frame.services.host.cpu_event) return;
    LARGE_INTEGER now{}; QueryPerformanceCounter(&now);
    rsf_game_cpu_event event{};
    rsf_skyrim_cpu_frame_event(&event, &frame.render, stage, frame.services.host.session_id, frame.source,
        uint64_t(now.QuadPart), frame.services.frequency);
    try { frame.services.host.cpu_event(frame.services.host.user, &event); } catch (...) {}
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
// The order matches the Rust admission table. MinHook writes a disabled trampoline first;
// a successful prepare pins these native detours, and stop leaves their continuations resident.
enum HookIndex : uint32_t {
    tick_hook,
    poll_hook,
    render_hook,
    dispatch_hook,
    world_hook,
    controls_hook,
    camera_update_hook,
    jobs_submit_hook,
    jobs_join_hook,
    named_poll_hook,
    movement_hook,
    player_update_hook,
    player_camera_update_hook,
    hook_count
};
static_assert(hook_count == 13);
const std::array<void*, hook_count> detours{
    reinterpret_cast<void*>(&tick),
    reinterpret_cast<void*>(&poll),
    reinterpret_cast<void*>(&render),
    reinterpret_cast<void*>(&dispatch),
    reinterpret_cast<void*>(&world),
    reinterpret_cast<void*>(&controls),
    reinterpret_cast<void*>(&camera_update),
    reinterpret_cast<void*>(&jobs_submit),
    reinterpret_cast<void*>(&jobs_join),
    reinterpret_cast<void*>(&named_poll),
    reinterpret_cast<void*>(&movement),
    reinterpret_cast<void*>(&player_update),
    reinterpret_cast<void*>(&player_camera_update),
};
std::array<std::atomic<void*>, hook_count> originals{};

const CpuHookLeaves cpu_hook_leaves{
    sizeof(CpuHookLeaves), 1,
    [](uint32_t index, uintptr_t target) noexcept {
        void* original{};
        if (MH_CreateHook(reinterpret_cast<void*>(target), detours[index], &original) != MH_OK) return false;
        originals[index].store(original, std::memory_order_release);
        return true;
    },
    [](uint32_t index, uintptr_t target) noexcept {
        const auto status = MH_RemoveHook(reinterpret_cast<void*>(target));
        if (status != MH_OK && status != MH_ERROR_NOT_CREATED) return false;
        originals[index].store(nullptr, std::memory_order_release);
        return true;
    },
    [](uintptr_t target) noexcept { return int32_t(MH_EnableHook(reinterpret_cast<void*>(target))); },
    [](uintptr_t target) noexcept {
        const auto status = MH_DisableHook(reinterpret_cast<void*>(target));
        return status == MH_OK || status == MH_ERROR_DISABLED;
    },
    []() noexcept {
        LARGE_INTEGER value{};
        return QueryPerformanceFrequency(&value) && value.QuadPart > 0 ? uint64_t(value.QuadPart) : uint64_t(0);
    },
};
static_assert(MH_OK == 0);

template<class Function> Function continuation(HookIndex index) noexcept {
    return reinterpret_cast<Function>(originals[index].load(std::memory_order_acquire));
}
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
bool read_poll_managers(uintptr_t base, uintptr_t job_manager, uintptr_t poll_manager) noexcept {
    ManagerState native{};
    uintptr_t input_manager{};
    return native.read() && native.manager == job_manager && native.requested <= 1 && native.running == 1 &&
        copy_input(&input_manager, reinterpret_cast<const void*>(base + 0x3204b30), sizeof(input_manager)) &&
        input_manager && input_manager == poll_manager;
}
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
    if (rsf_skyrim_cpu_active()) {
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
                    current->input.player_update_calls || current->input.movement_calls ||
                        current->input.camera_updates || current->input.player_camera_updates,
                    state == free_state); // the native test, including both null
                const auto stamp = input_ownership.snapshot();
                if (current->refusal != BindRefusal::none) {
                    if (prospective.release(current->source).source) {
                        abort_source(current->services, current->source, RSF_GAME_CPU_ABORT_NOT_CONSUMED);
                    }
                } else if (prospective.bind(current->source, stamp.generation)) {
                    rsf_skyrim_worker_bind(current->source);
                    emit_source(current->services, RSF_GAME_CPU_SOURCE_BIND, current->source, 0, 0, 0, 0, 0);
                } else if (prospective.release(current->source).source) {
                    abort_source(current->services, current->source, RSF_GAME_CPU_ABORT_STATE_CHANGED);
                }
            }
            // This verified normal-simulation branch starts actor/player work even when
            // prospective input ownership was refused. Report the engine fact independently.
            emit(*current, RSF_GAME_CPU_SIMULATION_BEGIN);
            // Original wakes the manager only after setting both native mode1 fields.
            // Publish immutable scalars first so a newly awakened worker sees them.
            ticket = job_ownership.publish(before.manager, current->source, current->services.host.session_id, true);
            current->jobs = ticket;
        } else if (current || job_ownership.snapshot().generation) {
            // Mid-tick activation has no owned source. Only a real outstanding ticket
            // or a refused owned submit invalidates provenance.
            job_ownership.invalidate();
        }
    }
    const auto result = continuation<uint32_t (*)()>(jobs_submit_hook)();
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
    const auto result = continuation<uint32_t (*)()>(jobs_join_hook)();
    if (owns_wait) {
        ManagerState after{};
        if (result == WAIT_OBJECT_0 && after.read() && after.manager == before.manager &&
            after.main == before.main && after.requested == 0 && after.running == 0) {
            (void)job_ownership.retire(ticket);
        } else job_ownership.invalidate();
    } else if (rsf_skyrim_cpu_active() && job_ownership.snapshot().generation) job_ownership.invalidate();
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
    if (rsf_skyrim_cpu_active() && caller == uintptr_t(GetModuleHandleW(nullptr)) + 0xebc248 &&
        !previous.generation && native.read() &&
        native.manager == ticket.manager && native.requested <= 1 && native.running == 1)
        named_poll_jobs = ticket;
    const auto result = continuation<uintptr_t (*)(void*)>(named_poll_hook)(data);
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
    if (!current || !current->services.host.render_pass || !cpu_render_identity(pass)) return pass;
    rsf_skyrim_cpu_render_fact(&pass, current->services.host.session_id);
    return pass;
}
void report_render(const rsf_game_render_pass& pass, uint32_t begin) {
    if (current && current->services.host.render_pass && pass.source_frame_id) {
        try { current->services.host.render_pass(current->services.host.user, nullptr, &pass, begin); } catch (...) {}
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
    const auto result = continuation<uintptr_t (*)(void*, uint32_t, uint8_t)>(world_hook)(main, output, flags);
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
    if (rsf_skyrim_cpu_active() && copy_input(&native_manager, base + 0x3204b30, sizeof(native_manager)) &&
        uintptr_t(manager) == native_manager) {
        if (const auto kinds = cpu_input_kinds(list)) {
            const auto services = cpu_services();
            host = services.host;
            LARGE_INTEGER now{}; QueryPerformanceCounter(&now);
            rsf_skyrim_cpu_input_event(&event, host.session_id, kinds, uint64_t(now.QuadPart), services.frequency);
        }
    }
    auto* previous_list = dispatched_list;
    dispatched_list = current_poll && current_poll->manager == uintptr_t(manager) ? list : nullptr;
    if (dispatched_list) rsf_skyrim_poll_record_kinds(current_poll, event.input_kind);
    const auto result = continuation<int (*)(void*, void*)>(dispatch_hook)(manager, list);
    dispatched_list = previous_list;
    // Worker polling is not assigned the latest tick. The runtime owns any bounded
    // association of this delivered input with an engine source and vendor policy.
    if (event.input_kind && host.cpu_event) {
        try { host.cpu_event(host.user, &event); } catch (...) {}
    }
    return result;
}
short tick(void* main)
{
    const auto original = continuation<short (*)(void*)>(tick_hook);
    if (!rsf_skyrim_cpu_active() || current) return original(main);
    Entry lease;
    if (!rsf_skyrim_cpu_active()) return original(main);
    Frame frame{}; frame.services = cpu_services(); frame.main = main;
    // The previous generation's late poll becomes this tick's identity only when it was
    // sampled with exclusive control state and its native job generation joined normally.
    // Its actual input therefore predates this FRAME_BEGIN; binding still waits for the
    // normal-simulation submit. Anything else is withdrawn before this frame begins.
    frame.reservation = prospective.adopt(frame.services.host.session_id, job_ownership.retired());
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
            abort_source(frame.services, withdrawn, RSF_GAME_CPU_ABORT_STATE_CHANGED);
        frame.source = rsf_skyrim_cpu_next_source();
    }
    current = &frame;
    emit(frame, RSF_GAME_CPU_FRAME_BEGIN);
    const auto result = original(main);
    if (frame.reservation.source) {
        // An adopted, unbound reservation (paused/frozen tick) closes unbound with FRAME_END.
        const auto owned = prospective.release(frame.source);
        if (owned.phase == ProspectiveSources::bound) rsf_skyrim_worker_release();
        const auto& facts = frame.reservation; // immutable adoption facts, including a refused bind
        if (frame.services.host.log && prospective_reports.fetch_add(1) < 16) {
            // PlayerCharacter update 0x745200 runs on this thread at 0x659f4d (via 0x659ea0, after
            // the submit) and drives PlayerCamera/camera-state updates; movement runs on workers.
            char message[560]{};
            WorkerCounts worker{};
            rsf_skyrim_worker_counts(&worker);
            std::snprintf(message, sizeof(message),
                "Skyrim prospective source=%llu predecessor=%llu poll_generation=%llu job_generation=%llu bound=%d refusal=%u kinds=0x%x main_player_update=%u main_camera=%u main_player_camera=%u main_movement=%u main_generation_match=%d main_ambiguous=%d worker_movement=%u worker_player_update=%u worker_camera=%u mismatched=%u; input timeline ownership diagnostics",
                static_cast<unsigned long long>(frame.source), static_cast<unsigned long long>(facts.predecessor),
                static_cast<unsigned long long>(facts.poll_generation), static_cast<unsigned long long>(facts.job_generation),
                owned.phase == ProspectiveSources::bound, static_cast<unsigned>(frame.refusal), facts.kinds,
                frame.input.player_update_calls, frame.input.camera_updates, frame.input.player_camera_updates,
                frame.input.movement_calls, frame.input.consumed.generation == facts.poll_generation, frame.input.ambiguous,
                worker.movement, worker.player_update, worker.camera, worker.mismatched);
            try { frame.services.host.log(frame.services.host.user, message); } catch (...) {}
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
    rsf_skyrim_poll_begin(&scope, uintptr_t(manager));
    current_poll = &scope;
    const bool running = rsf_skyrim_cpu_active();
    CpuPollProvenance provenance{};
    const bool scheduled = running && cpu_poll_provenance(provenance);
    const auto services = running ? cpu_services() : CpuServices{};
    if (running && poll_samples.fetch_add(1) < 16 && services.host.log) {
        char message[352]{};
        std::snprintf(message, sizeof(message),
            "Skyrim native poll thread=%lu scoped_source=%llu native_job_generation=%llu submitting_source=%llu poll_generation=%llu; submitting source is not input consumption",
            GetCurrentThreadId(), static_cast<unsigned long long>(current ? current->source : 0),
            static_cast<unsigned long long>(provenance.native_job_generation),
            static_cast<unsigned long long>(provenance.submitting_source_frame_id),
            static_cast<unsigned long long>(provenance.poll_generation));
        try { services.host.log(services.host.user, message); } catch (...) {}
    }
    uint64_t reserved{};
    if (running) {
        if (scheduled) {
            // The verified mode1 late poll of the submitting source's generation. Its consumer
            // is a later tick; reserve that future identity before the unchanged native call.
            const auto source = rsf_skyrim_cpu_next_source();
            const auto reservation = prospective.reserve(source, provenance.submitting_source_frame_id,
                provenance.session_id, provenance.native_job_generation, provenance.poll_generation);
            if (reservation.withdrawn) abort_source(services, reservation.withdrawn,
                reservation.overlap ? RSF_GAME_CPU_ABORT_OVERLAP : RSF_GAME_CPU_ABORT_INTERVENING_POLL);
            if (reservation.reserved.source) {
                reserved = source;
                emit_source(services, RSF_GAME_CPU_SOURCE_RESERVE, source, provenance.submitting_source_frame_id,
                    provenance.poll_generation, RSF_GAME_CPU_SOURCE_OVERLAPS_PREDECESSOR, 0, 0);
                // Withdrawal can win between local reservation and the host callback.
                // Repeat its terminal fact after RESERVE so the host cannot retain it open.
                if (!prospective.still_reserved(source)) {
                    abort_source(services, source, RSF_GAME_CPU_ABORT_STATE_CHANGED);
                    reserved = 0;
                }
            }
        } else {
            // Mode2, paused main-thread, nested or unverified polls keep native behavior and
            // replace the controls any pending reservation sampled.
            if (const auto withdrawn = prospective.withdraw_pending())
                abort_source(services, withdrawn, RSF_GAME_CPU_ABORT_INTERVENING_POLL);
            rsf_skyrim_worker_intervening_poll();
        }
    }
    // A scheduled late poll never samples the tick that happens to run it.
    if (current && !scheduled && rsf_skyrim_cpu_sample_tick(&current->render)) {
        emit(*current, RSF_GAME_CPU_INPUT_SAMPLE);
    }
    // The actual sample time: after the synchronous RESERVE callback (and anything the runtime
    // does inside it) has returned, immediately before the unchanged native poll.
    const auto original = continuation<uintptr_t (*)(void*, float)>(poll_hook);
    LARGE_INTEGER poll_begin{};
    if (reserved) QueryPerformanceCounter(&poll_begin);
    const auto result = original(manager, timestep);
    current_poll = previous_poll;
    rsf_skyrim_poll_end(&scope, previous_poll != nullptr);
    if (reserved) {
        // Produced only when this exact poll left the completed, unchanged control state.
        const auto sample = rsf_skyrim_poll_sample_status(&scope, reserved);
        if (sample == PollSample::not_produced) {
            abort_source(services, reserved, RSF_GAME_CPU_ABORT_NOT_PRODUCED);
        } else if (sample == PollSample::ready) {
            // Report before the adapter can adopt it, so the runtime never sees the frame first.
            emit_source(services, RSF_GAME_CPU_SOURCE_SAMPLED, reserved, 0, scope.ticket.generation,
                RSF_GAME_CPU_SOURCE_PRODUCED, scope.kinds, 0, uint64_t(poll_begin.QuadPart));
            if (!prospective.sample(reserved, true, scope.kinds) && prospective.withdraw_pending(reserved))
                abort_source(services, reserved, RSF_GAME_CPU_ABORT_STATE_CHANGED);
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
    const auto result = continuation<uintptr_t (*)(void*, void*, void*)>(controls_hook)(owner, list, source);
    uintptr_t singleton{};
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    if (rsf_skyrim_cpu_active() && current_poll && list && list == dispatched_list &&
        copy_input(&singleton, reinterpret_cast<const void*>(base + 0x31a5698), sizeof(singleton)) &&
        singleton == uintptr_t(owner)) rsf_skyrim_poll_sink_produced(current_poll);
    input_ownership.end_write();
    return result;
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
    continuation<void (*)(void*, float, void*, float*)>(movement_hook)(actor, timestep, argument, output);
    if (rsf_skyrim_cpu_active() && player && is_player(actor)) {
        if (current) rsf_skyrim_cpu_record_consumer(&current->input, &stamp, CpuConsumer::movement, true);
        else rsf_skyrim_worker_record_consumer(&stamp, CpuConsumer::movement);
    }
}
uintptr_t player_update(void* actor, float timestep)
{
    Entry lease;
    const auto stamp = input_ownership.snapshot();
    const bool player = is_player(actor);
    const auto result = continuation<uintptr_t (*)(void*, float)>(player_update_hook)(actor, timestep);
    if (rsf_skyrim_cpu_active() && player && is_player(actor)) {
        if (current) rsf_skyrim_cpu_record_consumer(&current->input, &stamp, CpuConsumer::player_update, true);
        else rsf_skyrim_worker_record_consumer(&stamp, CpuConsumer::player_update);
    }
    return result;
}
uintptr_t camera_update(void* state)
{
    Entry lease;
    const auto stamp = input_ownership.snapshot();
    uint8_t disabled{1};
    const bool consumes = copy_input(&disabled, static_cast<const unsigned char*>(state) + 0xe0, sizeof(disabled)) && !disabled;
    const auto result = continuation<uintptr_t (*)(void*)>(camera_update_hook)(state);
    if (rsf_skyrim_cpu_active() && current) {
        rsf_skyrim_cpu_record_consumer(&current->input, &stamp, CpuConsumer::camera, consumes);
    } else if (rsf_skyrim_cpu_active() && consumes) {
        rsf_skyrim_worker_record_consumer(&stamp, CpuConsumer::camera);
    }
    return result;
}
// TESCamera::Update(this) dispatches the current state vfunc at +0x18, with a void result.
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
    continuation<void (*)(void*)>(player_camera_update_hook)(camera);
    if (rsf_skyrim_cpu_active() && current && player_camera)
        rsf_skyrim_cpu_record_consumer(&current->input, &stamp, CpuConsumer::player_camera, true);
}
uintptr_t render(void* main)
{
    Entry lease;
    rsf_skyrim_cpu_render_receipt previous{};
    if (current) {
        const auto screen = rsf_skyrim_cpu_screen(uintptr_t(main), false, query_menus);
        if (rsf_skyrim_cpu_begin_render(&current->render, screen, &previous)) {
            emit(*current, RSF_GAME_CPU_SIMULATION_END);
        }
    }
    auto pass = render_fact(RSF_GAME_RENDER_FRAME);
    report_render(pass, 1);
    const auto result = continuation<uintptr_t (*)(void*)>(render_hook)(main);
    if (current) (void)cpu_render_identity(pass);
    report_render(pass, 0);
    if (current) rsf_skyrim_cpu_end_render(&current->render, &previous);
    return result;
}
}

bool cpu_prepare()
{
    return rsf_skyrim_cpu_rust_prepare(&cpu_hook_leaves);
}
bool cpu_bind(const rsf_game_host_services& host)
{
    return rsf_skyrim_cpu_rust_bind(&host);
}

bool cpu_start()
{
    CpuStartRefusal refusal{};
    if (rsf_skyrim_cpu_rust_start(&cpu_hook_leaves, &refusal)) return true;
    const auto services = cpu_services();
    if (services.host.log) {
        char message[180]{};
        std::snprintf(message, sizeof(message), "Skyrim CPU start refused: %s RVA=%X status=%d",
            refusal.stage, refusal.rva, refusal.status);
        services.host.log(services.host.user, message);
    }
    return false;
}
void cpu_quiesce() { rsf_skyrim_cpu_rust_quiesce(); }
bool cpu_disable() { return rsf_skyrim_cpu_rust_disable(&cpu_hook_leaves); }
bool cpu_stop() { return rsf_skyrim_cpu_rust_stop(&cpu_hook_leaves); }
bool cpu_busy() { return rsf_skyrim_cpu_rust_busy(); }
uint64_t cpu_source() { return current ? current->source : 0; }
bool cpu_render_identity(rsf_game_render_pass& pass) {
    return current && rsf_skyrim_cpu_render_identity(&current->render, &pass, current->source,
        rsf_skyrim_cpu_active());
}
bool cpu_rendered_source_eligible() noexcept {
    return current && rsf_skyrim_rendered_source_eligible(rsf_skyrim_cpu_active() && current->render.scope,
        current->render.primary_world_entries, current->render.screen);
}
void cpu_render_diagnostic(const char* stage, const rsf_game_render_pass& pass) {
    if (!current || !current->services.host.log || (current->source > 8 && current->source % 128 != 0)) return;
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
        unsigned(rsf_skyrim_cpu_active()), frame.render.primary_world_entries, unsigned(cpu_rendered_source_eligible()),
        unsigned(frame.render.render_entered), unsigned(frame.render.simulation_ended), frame.render.primary_count,
        static_cast<unsigned long long>(frame.reservation.source), static_cast<unsigned long long>(owned.source), unsigned(owned.phase),
        unsigned(owned.source == frame.source && owned.phase == ProspectiveSources::bound), unsigned(frame.refusal),
        static_cast<unsigned long long>(frame.reservation.poll_generation), static_cast<unsigned long long>(frame.reservation.job_generation),
        unsigned(frame.entry_readable), unsigned(frame.queued_move), unsigned(frame.input.ambiguous));
    try { frame.services.host.log(frame.services.host.user, message); } catch (...) {}
}
uint32_t cpu_input_kinds(const void* list) noexcept {
    return rsf_skyrim_cpu_input_kinds(uintptr_t(list), [](uintptr_t address, void* out, size_t size) noexcept {
        return copy_input(out, reinterpret_cast<const void*>(address), size);
    });
}
bool cpu_input_consumption(CpuInputConsumption& result) noexcept {
    result = {};
    if (!rsf_skyrim_cpu_active() || !current ||
        !rsf_skyrim_cpu_consumption_proven(&current->input)) return false;
    result.source_frame_id = current->source;
    result.poll_generation = current->input.consumed.generation;
    result.camera_updates = current->input.camera_updates;
    result.movement_calls = current->input.movement_calls;
    result.player_update_calls = current->input.player_update_calls;
    return true;
}
bool cpu_poll_provenance(CpuPollProvenance& result) noexcept {
    result = {};
    CpuPollProvenance proof{};
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    if (!rsf_skyrim_cpu_active() || !current_poll ||
        !rsf_skyrim_poll_provenance(&proof, current_poll, &named_poll_jobs, base, read_poll_managers) ||
        !rsf_skyrim_cpu_active()) return false;
    result = proof;
    return true;
}
}
