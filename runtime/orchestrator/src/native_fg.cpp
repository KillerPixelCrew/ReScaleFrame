// SPDX-License-Identifier: MIT
#include <rescaleframe/native_fg.h>
#include <rescaleframe/game_api.h>
#include "latency_antilag.h"
#include "latency_policy.h"
#include "input_receipts.h"
#include "native_fg_platform.h"
#include <dxgi1_6.h>
#include <rescaleframe/native_cpu.h>
#include <d3d12.h>
#include <wrl/client.h>
#include <array>
#include <atomic>
#include <mutex>
#include <cstring>
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <iterator>

namespace {
using Microsoft::WRL::ComPtr;
// Graphics-side options, status and the log sink are owned by native_fg.rs; this CPU owner reads them.
rsf_native_fg_options options() {
    rsf_native_fg_options value{}; rsf_core_native_fg_options(&value); return value;
}
std::mutex trace_guard;
uint32_t debug_lines = 0;
uint64_t last_error_qpc = 0;
struct Sink {
    rsf_backend_log_fn fn = nullptr; void* user = nullptr;
    explicit operator bool() const { return fn != nullptr; }
    void operator()(const char* text) const { if (fn) fn(user, text); }
};
// debug_only returns nothing unless debug output was requested.
Sink current_sink(bool debug_only) {
    if (debug_only && !options().debug) return {};
    Sink sink; rsf_core_native_fg_logger(&sink.fn, &sink.user); return sink;
}
enum class FailureStage : uint32_t {
    SrPrerequisite, SrToken, SrInitialize, PaceDependency, PaceSelection, PaceCall,
    RenderCpu, RenderProspective, RenderInput, RenderDerivedEnd, RenderRecheck, RenderMarkers, RenderProvider, SrSharedEvaluate, RenderOwner
};
// One receipt per named failure stage for this module lifetime, independent of debug.
// Unknown metadata stays explicitly unknown; diagnostics never query native/vendor state.
void failure_receipt(FailureStage stage, uint64_t source, uint64_t session, int32_t result,
    uint32_t detail, uint64_t generation = 0, const rsf_native_cpu_frame* cpu = nullptr,
    const rsf_native_cpu_reservation* reservation = nullptr) {
    static std::atomic<uint32_t> reported{};
    const uint32_t bit = 1u << uint32_t(stage);
    if (reported.load(std::memory_order_relaxed) & bit) return;
    const auto sink = current_sink(false);
    if (!sink.fn || (reported.fetch_or(bit, std::memory_order_relaxed) & bit)) return;
    static constexpr const char* names[]{"sr-prerequisite", "sr-token", "sr-initialize", "pace-dependency",
        "pace-selection", "pace-call", "render-cpu", "render-prospective", "render-input", "render-derived-end",
        "render-recheck", "render-markers", "render-provider", "sr-shared-evaluate", "render-owner"};
    char text[600]{};
    std::snprintf(text, sizeof(text),
        "Native failure stage=%s source=%llu session=%llu session_known=%u result=%d detail=0x%x generation_known=%u generation=%llu "
        "cpu_known=%u cpu_mask=%u marker_mask=%u cpu_flags=%u cpu_failed=%u reservation_known=%u reservation_state=%u predecessor_present=%d",
        names[uint32_t(stage)], static_cast<unsigned long long>(source), static_cast<unsigned long long>(session),
        uint32_t(session != 0), result, detail, uint32_t(generation != 0), static_cast<unsigned long long>(generation), uint32_t(cpu != nullptr),
        cpu ? cpu->stage_mask : 0, cpu ? cpu->marker_mask : 0, cpu ? cpu->flags : 0, cpu ? cpu->failed : 0,
        uint32_t(reservation != nullptr), reservation ? reservation->state : 0, reservation ? reservation->predecessor_present : 0);
    sink.fn(sink.user, text);
}
void trace(const char* stage, uint64_t id, int32_t result) {
    const bool debug = options().debug != 0;
    bool due = false;
    { std::lock_guard<std::mutex> lock(trace_guard);
      bool error_due = false;
      if (result != RSF_BACKEND_OK) {
          LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
          error_due = uint64_t(now.QuadPart) - last_error_qpc >= uint64_t(frequency.QuadPart);
          if (error_due) last_error_qpc = uint64_t(now.QuadPart);
      }
      due = debug && (debug_lines++ < 256 || error_due); }
    if (!due) return;
    if (const auto sink = current_sink(false)) { char text[160]; std::snprintf(text, sizeof(text), "FG/Reflex %s source=%llu result=%d", stage,
        static_cast<unsigned long long>(id), result); sink(text); }
}
// The game's screen class decides, through the one rule every generation consumer applies.
bool generates_on(uint32_t screen) {
    rsf_frame_record record{}; record.struct_size = sizeof(record); record.screen = screen;
    return rsf_frame_allows_fg(&record) != 0;
}
thread_local uint64_t rhi_frame_source = 0;
// Default order: the game thread sleeps once at its pacing boundary, after the presenting
// thread finished the previous frame's Present and before this frame's input. Nonzero keeps
// UE's one-frame overlap instead, where that sleep races the previous submit/Present markers.
std::atomic<uint32_t> threaded_reflex{0};
std::atomic<uint32_t> graphics_api{11};
std::atomic<uint32_t> rendered_source{0};
std::atomic<uint64_t> derived_boundaries{0};
// Highest source frame whose Present returned, or which ended or was abandoned without one.
std::atomic<uint64_t> presented_through{0};
std::atomic<uint64_t> last_begun{0};
std::atomic<uint32_t> join_skip{0};
HANDLE completion_event() { static const HANDLE value = CreateEventW(nullptr, FALSE, FALSE, nullptr); return value; }
void complete_frame(uint64_t id) {
    auto seen = presented_through.load(std::memory_order_relaxed);
    while (seen < id && !presented_through.compare_exchange_weak(seen, id)) {}
    if (const auto event = completion_event()) SetEvent(event);
}
uint64_t now_qpc() { LARGE_INTEGER now{}; QueryPerformanceCounter(&now); return uint64_t(now.QuadPart); }
rsf_backend_result frame_acquire(uint64_t id);
rsf_backend_result frame_marker(uint64_t id, rsf_latency_marker marker, bool* marked = nullptr) {
    if (marked) *marked = false;
    if (rendered_source.load()) { if (marked) *marked = true; return RSF_BACKEND_OK; }
    const auto accepted = rsf_native_cpu_marker(id,uint32_t(marker));
    if (accepted <= 0) return accepted == 0 ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const auto result = rsf_d3d11_present_marker(rsf_d3d11_present_generation(), id, marker, 0);
    if (marked) *marked = result == RSF_BACKEND_OK;
    return result;
}
rsf_backend_result frame_abort(uint64_t id) { return rsf_d3d11_present_abort(id); }
bool frame_live(rsf_streamline_host* host, uint64_t id) {
    return host ? rsf_streamline_host_token(host, id) != nullptr : id && presented_through.load() < id;
}
bool frame_presented(rsf_streamline_host* host, uint64_t id) {
    return host ? rsf_streamline_host_presented(host, id) != 0 : presented_through.load() >= id;
}
// Debug only. While the primary scene submission of one frame stays open far longer than a
// frame, a watcher samples where the render thread is: its instruction pointer and the code
// addresses on its stack, as module+offset. The thread is suspended only for a register read
// and a bounded stack copy; nothing that can take a lock runs until it is resumed.
std::atomic<uint64_t> submission_opened_qpc{0}, submission_opened_id{0};
std::atomic<HANDLE> sampled_thread{nullptr};
std::atomic<uint32_t> hitch_watch_started{0};
size_t describe_address(uint64_t address, char* text, size_t size) {
    HMODULE module = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            reinterpret_cast<LPCWSTR>(uintptr_t(address)), &module) || !module) return 0;
    MEMORY_BASIC_INFORMATION region{};
    if (!VirtualQuery(reinterpret_cast<void*>(uintptr_t(address)), &region, sizeof(region)) ||
        !(region.Protect & (PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY))) return 0;
    wchar_t path[MAX_PATH]{}; GetModuleFileNameW(module, path, MAX_PATH);
    const wchar_t* leaf = path; for (const wchar_t* c = path; *c; ++c) if (*c == L'\\') leaf = c + 1;
    const int written = std::snprintf(text, size, " %ls+0x%llx", leaf,
        static_cast<unsigned long long>(address - uint64_t(uintptr_t(module))));
    return written > 0 && size_t(written) < size ? size_t(written) : 0;
}
DWORD WINAPI hitch_watch(void*) {
    LARGE_INTEGER frequency{}; QueryPerformanceFrequency(&frequency);
    uint64_t current = 0; uint32_t taken = 0, total = 0;
    static uint64_t words[512];
    while (total < 36) {
        Sleep(2);
        const auto opened = submission_opened_qpc.load(std::memory_order_acquire);
        const auto id = submission_opened_id.load(std::memory_order_acquire);
        const auto thread = sampled_thread.load(std::memory_order_acquire);
        if (!opened || !thread) continue;
        if (id != current) { current = id; taken = 0; }
        const double open_ms = double(now_qpc() - opened) * 1000.0 / double(frequency.QuadPart);
        if (taken >= 3 || open_ms < 15.0 * (taken + 1)) continue;
        uint64_t rip = 0; size_t count = 0;
        if (SuspendThread(thread) == DWORD(-1)) continue;
        CONTEXT context{}; context.ContextFlags = CONTEXT_CONTROL;
        if (GetThreadContext(thread, &context)) {
            rip = context.Rip;
            MEMORY_BASIC_INFORMATION region{};
            if (VirtualQuery(reinterpret_cast<void*>(uintptr_t(context.Rsp)), &region, sizeof(region)) &&
                region.State == MEM_COMMIT) {
                const auto end = uint64_t(uintptr_t(region.BaseAddress)) + region.RegionSize;
                count = std::min<size_t>(std::size(words), size_t((end - context.Rsp) / 8));
                std::memcpy(words, reinterpret_cast<void*>(uintptr_t(context.Rsp)), count * 8);
            }
        }
        ResumeThread(thread);
        ++taken; ++total;
        char text[1900]; size_t used = size_t(std::snprintf(text, sizeof(text),
            "Render hitch source=%llu open %.1f ms; executing", static_cast<unsigned long long>(id), open_ms));
        used += describe_address(rip, text + used, sizeof(text) - used);
        used += size_t(std::snprintf(text + used, sizeof(text) - used, "; stack code addresses:"));
        uint32_t listed = 0;
        for (size_t i = 0; i < count && listed < 40 && used + 96 < sizeof(text); ++i) {
            const auto added = describe_address(words[i], text + used, sizeof(text) - used);
            if (added) { used += added; ++listed; }
        }
        current_sink(false)(text);
    }
    return 0;
}
void watch_submission(uint64_t id, uint32_t screen, bool open) {
    if (!open) { submission_opened_qpc.store(0, std::memory_order_release); return; }
    if (!options().debug) return;
    // Loading and screen changes are slow by nature and used up every sample. Arm only once a
    // generating screen has been showing for three seconds. Render thread only.
    static uint32_t shown = UINT32_MAX; static uint64_t since = 0;
    if (screen != shown) { shown = screen; since = GetTickCount64(); }
    if (!generates_on(screen) || GetTickCount64() - since < 3000) return;
    if (!sampled_thread.load(std::memory_order_acquire)) {
        HANDLE duplicate = nullptr;
        if (DuplicateHandle(GetCurrentProcess(), GetCurrentThread(), GetCurrentProcess(), &duplicate,
                THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION, FALSE, 0))
            sampled_thread.store(duplicate, std::memory_order_release);
    }
    if (!hitch_watch_started.exchange(1)) {
        if (const HANDLE watcher = CreateThread(nullptr, 0, hitch_watch, nullptr, 0, nullptr)) CloseHandle(watcher);
    }
    submission_opened_id.store(id, std::memory_order_release);
    submission_opened_qpc.store(now_qpc(), std::memory_order_release);
}
struct CpuOwner {
    uint64_t provider_generation = 0, session = 0;
    uint64_t id = 0;
    bool submitted = false, ended = false, failed = false, render_complete = false;
    bool paced = false, pacing_started = false, pacing_finished = false;
    bool prospective = false;
    bool render_started = false, render_starting = false, submit_ending = false;
    bool present_started = false, present_starting = false, present_ended = false, present_ending = false;
    bool audited = false;
    uint32_t expected_count = 0, scene_begins = 0, scene_ends = 0, sr_calls = 0, presents = 0, windows = 0;
    uint32_t rhi_begins = 0, rhi_ends = 0, screen = 0, native_frame = 0, mismatched_window = 0;
    uint64_t first_submission = 0, last_submission = 0, first_view = 0, last_view = 0;
    uint64_t join_ticks = 0, reflex_ticks = 0, ready_qpc = 0, submit_begin = 0, submit_end = 0;
    uint64_t previous = 0, source_qpc = 0, source_frequency = 0;
    float source_delta_ms = 0;
    bool submit_ended = false;
};
std::mutex cpu_guard;
rsf_native_latency_window receipt_window{};
uint32_t receipt_thread{};
rsf_native_input_routes receipt_routes{sizeof(receipt_routes),1};
std::array<CpuOwner, 128> cpu_frames{};
// Runs f on the ring entry for id under cpu_guard. False when the entry holds another frame.
template<class F> bool with_frame(uint64_t id, F&& f) {
    std::lock_guard<std::mutex> lock(cpu_guard);
    auto& frame = cpu_frames[id % cpu_frames.size()];
    if (frame.id != id) return false;
    f(frame); return true;
}
rsf_backend_result frame_acquire(uint64_t id) {
    if (!id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto result=rsf_d3d11_present_acquire(id);
    auto* host=rsf_d3d11_present_host();
    if (result!=RSF_BACKEND_ERROR_NOT_READY || !host) return result;
    uint64_t oldest=0;
    {
        std::lock_guard lock(cpu_guard);
        for (const auto& frame:cpu_frames) {
            if (!frame.id || frame.id>=id || frame.failed || frame.render_started || frame.submitted ||
                (frame.pacing_started && !frame.pacing_finished)) continue;
            rsf_native_cpu_frame facts{};facts.struct_size=sizeof(facts);
            if (rsf_native_cpu_read(frame.session,frame.id,&facts) && facts.ended && (!oldest || frame.id<oldest)) oldest=frame.id;
        }
    }
    if (oldest && rsf_streamline_host_abort(host,oldest)==RSF_BACKEND_OK) {
        with_frame(oldest,[](CpuOwner& frame) {frame.failed=true;});
        ++derived_boundaries;
        trace("unsubmitted latency source retired at token capacity",oldest,RSF_BACKEND_ERROR_NOT_READY);
        result=rsf_d3d11_present_acquire(id);
    }
    return result;
}
struct OwnershipTotals {
    uint64_t epoch = 0, frames = 0, expected = 0, scenes = 0, ends = 0, sr = 0, presents = 0, windows = 0;
    uint64_t unpresented = 0, multi = 0, mismatched = 0, native_gaps = 0, rhi_begin = 0, rhi_end = 0;
    uint32_t previous_native = 0;
};
std::array<OwnershipTotals, 9> ownership{};
void audit_frame(uint64_t id) {
    char text[600]{};
    {
        std::lock_guard<std::mutex> lock(cpu_guard);
        auto& frame = cpu_frames[id % cpu_frames.size()];
        if (frame.id != id || frame.audited || !frame.ended || !frame.rhi_ends) return;
        frame.audited = true;
        auto& total = ownership[std::min(frame.screen, 8u)];
        LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
        if (!total.epoch) total.epoch = uint64_t(now.QuadPart);
        ++total.frames; total.expected += frame.expected_count; total.scenes += frame.scene_begins;
        total.ends += frame.scene_ends; total.sr += frame.sr_calls; total.presents += frame.presents;
        total.windows += frame.windows; total.rhi_begin += frame.rhi_begins; total.rhi_end += frame.rhi_ends;
        total.unpresented += frame.presents == 0; total.mismatched += frame.mismatched_window;
        total.multi += frame.scene_begins > 1 || frame.sr_calls > 1 || frame.presents > 1;
        if (frame.native_frame && total.previous_native && frame.native_frame != total.previous_native + 1) ++total.native_gaps;
        if (frame.native_frame) total.previous_native = frame.native_frame;
        if (uint64_t(now.QuadPart) - total.epoch < uint64_t(frequency.QuadPart)) return;
        std::snprintf(text, sizeof(text),
            "FG ownership screen=%u source=%llu native=%u frames=%llu expected=%llu scene_begin=%llu scene_end=%llu sr=%llu presents=%llu windows=%llu rhi_begin=%llu rhi_end=%llu no_present=%llu multi=%llu window_mismatch=%llu native_gaps=%llu submission=%llu..%llu view=%llx..%llx",
            frame.screen, static_cast<unsigned long long>(id), frame.native_frame,
            static_cast<unsigned long long>(total.frames), static_cast<unsigned long long>(total.expected),
            static_cast<unsigned long long>(total.scenes), static_cast<unsigned long long>(total.ends),
            static_cast<unsigned long long>(total.sr), static_cast<unsigned long long>(total.presents),
            static_cast<unsigned long long>(total.windows), static_cast<unsigned long long>(total.rhi_begin),
            static_cast<unsigned long long>(total.rhi_end), static_cast<unsigned long long>(total.unpresented),
            static_cast<unsigned long long>(total.multi), static_cast<unsigned long long>(total.mismatched),
            static_cast<unsigned long long>(total.native_gaps), static_cast<unsigned long long>(frame.first_submission),
            static_cast<unsigned long long>(frame.last_submission), static_cast<unsigned long long>(frame.first_view),
            static_cast<unsigned long long>(frame.last_view));
        const auto previous = total.previous_native; total = {}; total.previous_native = previous;
        total.epoch = uint64_t(now.QuadPart);
    }
    if (text[0]) current_sink(true)(text);
}
// Debug only. Averages and peaks of one second of source frames, from the CPU ledger and the
// graphics owner's SR wait and evaluation spans.
void report_frame_timing(const rsf_native_cpu_frame& cpu, const rsf_nfg_cpu_owner& owner, uint32_t screen,
    uint64_t wait_ticks, uint64_t evaluate_ticks) {
    if (!cpu.qpc_frequency || !owner.ready_qpc || !owner.submit_begin || owner.submit_end < owner.submit_begin) return;
    struct Timing {
        uint64_t start = 0, frames = 0;
        std::array<double, 8> sum{}, peak{};
    };
    static thread_local Timing timing;
    const auto now = now_qpc();
    if (!timing.start) timing.start = now;
    const auto elapsed_ticks = [](uint64_t end, uint64_t begin) { return end >= begin ? end - begin : 0; };
    const uint64_t spans[]{owner.join_ticks, owner.prospective && !owner.paced ? 0 : owner.reflex_ticks,
        elapsed_ticks(cpu.timestamps_qpc[RSF_GAME_CPU_INPUT_SAMPLE], owner.ready_qpc),
        elapsed_ticks(cpu.timestamps_qpc[RSF_GAME_CPU_SIMULATION_BEGIN], cpu.timestamps_qpc[RSF_GAME_CPU_INPUT_SAMPLE]),
        elapsed_ticks(cpu.timestamps_qpc[RSF_GAME_CPU_SIMULATION_END], cpu.timestamps_qpc[RSF_GAME_CPU_SIMULATION_BEGIN]),
        owner.submit_end - owner.submit_begin, evaluate_ticks, wait_ticks};
    const double scale = 1000.0 / double(cpu.qpc_frequency);
    for (size_t i = 0; i < timing.sum.size(); ++i) {
        const auto ms = double(spans[i]) * scale; timing.sum[i] += ms; timing.peak[i] = std::max(timing.peak[i], ms);
    }
    ++timing.frames;
    if (double(now - timing.start) * scale < 1000.0) return;
    if (const auto sink = current_sink(false)) {
        LASTINPUTINFO input{sizeof(input), 0};
        const uint32_t idle = GetLastInputInfo(&input) ? uint32_t(GetTickCount() - input.dwTime) : UINT32_MAX;
        char text[640];
        std::snprintf(text, sizeof(text),
            "FG source timing source=%llu frames=%llu screen=%u windows_input_idle_ms=%u join_ms=%.2f/%.2f pacing_operation_ms=%.2f/%.2f engine_preinput_ms=%.2f/%.2f input_to_sim_ms=%.2f/%.2f simulation_ms=%.2f/%.2f rhi_ms=%.2f/%.2f sr_ms=%.2f/%.2f sr_wait_ms=%.2f/%.2f",
            static_cast<unsigned long long>(owner.id), static_cast<unsigned long long>(timing.frames), screen, idle,
            timing.sum[0] / double(timing.frames), timing.peak[0], timing.sum[1] / double(timing.frames), timing.peak[1],
            timing.sum[2] / double(timing.frames), timing.peak[2], timing.sum[3] / double(timing.frames), timing.peak[3],
            timing.sum[4] / double(timing.frames), timing.peak[4], timing.sum[5] / double(timing.frames), timing.peak[5],
            timing.sum[6] / double(timing.frames), timing.peak[6], timing.sum[7] / double(timing.frames), timing.peak[7]);
        sink(text);
    }
    timing = {}; timing.start = now;
}
}
namespace {
// Game thread, which is also the window thread. Sent messages stay answered so a Present that
// calls into the window cannot wait on this thread; posted input is left for the engine's own
// pump after the sleep. A missing completion costs one bounded wait, then joins pause.
uint64_t join_previous_present(uint64_t previous) {
    if (!previous || presented_through.load(std::memory_order_acquire) >= previous) return 0;
    if (join_skip.load(std::memory_order_relaxed)) { join_skip.fetch_sub(1, std::memory_order_relaxed); return 0; }
    const auto event = completion_event(); if (!event) return 0;
    const auto started = now_qpc();
    const auto deadline = GetTickCount64() + 20;
    while (presented_through.load(std::memory_order_acquire) < previous) {
        const auto now = GetTickCount64();
        if (now >= deadline) {
            join_skip.store(120, std::memory_order_relaxed);
            trace("previous Present did not complete; joins paused", previous, RSF_BACKEND_ERROR_NOT_READY);
            break;
        }
        if (MsgWaitForMultipleObjectsEx(1, &event, DWORD(deadline - now), QS_SENDMESSAGE, 0) == WAIT_OBJECT_0 + 1) {
            MSG message{}; PeekMessageW(&message, nullptr, 0, 0, PM_NOREMOVE | PM_QS_SENDMESSAGE);
        }
    }
    return now_qpc() - started;
}
}
static void log_fg_stage(const char* stage, uint64_t source, uint64_t native_frame, int32_t result,
    const rsf_nfg_cpu_owner* owner = nullptr, const rsf_game_render_pass* window = nullptr) {
    if (options().mode == RSF_FG_OFF) return;
    const auto sink = current_sink(false);
    if (!sink || !stage) return;
    static std::mutex receipt_guard;
    static uint64_t previous_generation{};
    static uint32_t lines{}, stage_lines{}, prepare_lines{}, prepare_calls{};
    const auto generation = rsf_d3d11_present_generation();
    { std::lock_guard lock(receipt_guard);
      if (generation != previous_generation) {
          previous_generation = generation; lines = stage_lines = prepare_lines = prepare_calls = 0;
      }
      // Reserve rows for matching late-switch producer samples, not just startup.
      if (owner) {
          const bool first = prepare_calls++ < 8;
          if ((!first && (!source || source % 128)) || prepare_lines >= 64) return;
          ++prepare_lines;
      } else if (stage_lines++ >= 32) return;
      if (lines++ >= 96) return; }
    char text[1536]{}; std::snprintf(text, sizeof(text),
        "FG stage=%s generation=%llu source=%llu native_frame=%llu result=%d thread=%lu",
        stage, static_cast<unsigned long long>(generation), static_cast<unsigned long long>(source),
        static_cast<unsigned long long>(native_frame), result, static_cast<unsigned long>(GetCurrentThreadId()));
    if (owner && window) {
        // These independent read-only snapshots are diagnostic, not admission.
        // Read only after acquiring a row in the existing per-generation budget.
        rsf_native_cpu_frame cpu{}; cpu.struct_size = sizeof(cpu);
        const bool cpu_read = window->source_frame_id && rsf_native_cpu_read(window->session_id,window->source_frame_id,&cpu);
        rsf_native_cpu_reservation reservation{sizeof(reservation)};
        const bool reservation_read = window->source_frame_id && rsf_native_cpu_reservation_read(
            window->session_id,window->source_frame_id,&reservation);
        const auto used = std::strlen(text);
        std::snprintf(text+used,sizeof(text)-used,
            " owner_id=%llu owner_session=%llu window_session=%llu submitted=%u render_complete=%u paced=%u prospective=%u owner_failed=%u owner_generation=%llu provider_generation=%llu "
            "cpu_read=%u cpu_failed=%u cpu_mask=%u cpu_flags=%u reservation_read=%u reservation_state=%u reservation_abort=%u predecessor_present=%d poll_begin=%llu owner_ready=%llu",
            static_cast<unsigned long long>(owner->id), static_cast<unsigned long long>(owner->session),
            static_cast<unsigned long long>(window->session_id), uint32_t(owner->submitted), uint32_t(owner->render_complete),
            uint32_t(owner->paced), uint32_t(owner->prospective), uint32_t(owner->failed),
            static_cast<unsigned long long>(owner->provider_generation),
            static_cast<unsigned long long>(generation), uint32_t(cpu_read), cpu.failed, cpu.stage_mask, cpu.flags,
            uint32_t(reservation_read), reservation.state, reservation.abort_reason, reservation.predecessor_present,
            static_cast<unsigned long long>(reservation.poll_begin_qpc),
            static_cast<unsigned long long>(owner->ready_qpc));
    }
    sink(text);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_stage(const char* stage, uint64_t source, uint64_t native_frame, int32_t result) {
    log_fg_stage(stage,source,native_frame,result);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_rendered_source_set(uint32_t enabled) {
    rendered_source.store(enabled != 0);
    rsf_native_cpu_rendered_source_set(enabled);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_reflex_ordering_set(uint32_t threaded) {
    threaded_reflex.store(threaded ? 1u : 0u, std::memory_order_relaxed);
}
namespace {
struct AntiLagOwner {
    std::mutex guard;
    std::unique_ptr<rsf::latency::AntiLagDriver> driver{rsf::latency::make_antilag_driver()};
    rsf::latency::AntiLag context{*driver};
    void* device{};
    uint32_t vendor{}, pacing{}, reflex_mode{}, reflex_limit{};
    rsf_streamline_host* reflex_host{};
    bool reflex_configured{},denied{};
};
AntiLagOwner& anti_lag() { static auto* value = new AntiLagOwner; return *value; }
uint32_t select_cpu_pacing(uint64_t id, uint32_t mode, uint32_t limit) {
    rsf_streamline_graphics graphics{}; graphics.struct_size=sizeof(graphics);
    if (!rsf_d3d11_present_graphics(&graphics) || !graphics.native_device) return RSF_PACING_NONE;
    auto& owner = anti_lag();
    std::lock_guard lock(owner.guard);
    if (owner.device != graphics.native_device) {
        owner.device = graphics.native_device; owner.vendor=0;
        ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter1> adapter;
        if (SUCCEEDED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) &&
            SUCCEEDED(factory->EnumAdapterByLuid(static_cast<ID3D12Device*>(owner.device)->GetAdapterLuid(), IID_PPV_ARGS(&adapter)))) {
            DXGI_ADAPTER_DESC1 desc{};
            if (SUCCEEDED(adapter->GetDesc1(&desc)) && !(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) owner.vendor=desc.VendorId;
        }
    }
    rsf_fg_status state{}; state.struct_size=sizeof(state);
    rsf_d3d11_present_status snapshot{sizeof(snapshot),RSF_PRESENT_STATUS_VERSION};
    const bool status_known = rsf_d3d11_present_status_get(&snapshot) == RSF_BACKEND_OK;
    if (status_known) state = snapshot.state;
    const auto backend = status_known ? snapshot.backend : rsf_d3d11_present_backend();
    owner.context.bind(graphics_api.load() == 12 || backend == RSF_FG_BACKEND_FSR3 || backend == RSF_FG_BACKEND_FSR4 ? 12u : 11u, owner.device, owner.vendor);
    auto* host=rsf_d3d11_present_host();
    rsf_streamline_latency_status reflex{}; reflex.struct_size=sizeof(reflex);
    const bool reflex_available=host && rsf_streamline_host_latency_status(host,&reflex)==RSF_BACKEND_OK &&
        reflex.low_latency_available && reflex.profile!=RSF_SL_PROFILE_PCL;
    owner.pacing=rsf::latency::pacing_owner(state.pacing_owner,reflex_available,owner.context.state().available);
    owner.denied=owner.pacing!=RSF_PACING_NONE && !rsf_native_latency_claim(1);
    if (owner.denied) owner.pacing=RSF_PACING_NONE;
    (void)owner.context.pace(id,mode,limit,owner.denied ? RSF_PACING_REFLEX : owner.pacing==RSF_PACING_ANTILAG2 ? RSF_PACING_NONE : owner.pacing);
    if (owner.pacing==RSF_PACING_ANTILAG2 && !owner.context.state().available) owner.pacing=RSF_PACING_NONE;
    // DLSS-G's existing configuration uses eligibility to set its required Reflex mode and present ratio.
    // Other providers get the same host pacer independently of their generation implementation.
    if (reflex_available && backend!=RSF_FG_BACKEND_DLSS) {
        const uint32_t wanted=owner.pacing==RSF_PACING_REFLEX ? mode : RSF_REFLEX_OFF;
        const uint32_t presents=state.effective_mode==RSF_FG_FIXED ? state.effective_generated_frames+1 : 1;
        const uint32_t interval=owner.pacing==RSF_PACING_REFLEX ? limit/presents : 0;
        if (!owner.reflex_configured || owner.reflex_host!=host || owner.reflex_mode!=wanted || owner.reflex_limit!=interval) {
            const auto configured=rsf_streamline_host_reflex(host,wanted,interval);
            owner.reflex_configured=configured==RSF_BACKEND_OK;
            owner.reflex_host=host; owner.reflex_mode=wanted; owner.reflex_limit=interval;
        }
    } else owner.reflex_configured=false;
    return owner.pacing;
}
void anti_lag_status(rsf_fg_status& status) {
    auto& owner=anti_lag(); std::lock_guard lock(owner.guard);
    const auto state=owner.context.state();
    if (owner.pacing==RSF_PACING_NONE) {
        status.pacing_owner=RSF_PACING_NONE;status.low_latency_available=0;
        status.effective_reflex=RSF_REFLEX_OFF;status.latency_paced_frames=0;
    }
    if (owner.pacing==RSF_PACING_REFLEX) {
        rsf_streamline_latency_status reflex{}; reflex.struct_size=sizeof(reflex);
        if (auto* host=rsf_d3d11_present_host(); host && rsf_streamline_host_latency_status(host,&reflex)==RSF_BACKEND_OK) {
            status.pacing_owner=RSF_PACING_REFLEX; status.low_latency_available=reflex.low_latency_available;
            status.effective_reflex=reflex.reflex_mode; status.latency_paced_frames=reflex.sleep_calls;
        }
    }
    if (owner.pacing==RSF_PACING_XELL) {
        rsf_fg_status current{}; current.struct_size=sizeof(current);
        rsf_d3d11_present_status snapshot{sizeof(snapshot),RSF_PRESENT_STATUS_VERSION};
        if (rsf_d3d11_present_status_get(&snapshot)==RSF_BACKEND_OK) {
            current=snapshot.state;
            status.pacing_owner=RSF_PACING_XELL; status.low_latency_available=current.low_latency_available;
            status.effective_reflex=current.effective_reflex; status.latency_paced_frames=current.latency_paced_frames;
        }
    }
    if (owner.pacing==RSF_PACING_ANTILAG2 && state.available) {
        status.pacing_owner=RSF_PACING_ANTILAG2; status.low_latency_available=1;
        status.effective_reflex=state.enabled ? RSF_REFLEX_ON : RSF_REFLEX_OFF;
        status.latency_paced_frames=state.updates;
    }
}
void anti_lag_present(uint64_t id, void* chain) {
    auto& owner=anti_lag(); std::lock_guard lock(owner.guard);
    const auto backend=rsf_d3d11_present_backend();
    (void)owner.context.end_render(id,chain,backend==RSF_FG_BACKEND_FSR3 || backend==RSF_FG_BACKEND_FSR4);
}
}
namespace {
void pace_cpu_frame(uint64_t id, const rsf_native_cpu_reservation* reservation = nullptr) {
    const bool threaded = threaded_reflex.load(std::memory_order_relaxed) != 0;
    auto* host = rsf_d3d11_present_host();
        uint64_t previous = 0;
        bool repeat = true;
        with_frame(id, [&](CpuOwner& frame) { repeat = frame.pacing_started; frame.pacing_started = true; });
        if (repeat) return;
        with_frame(id, [&](const CpuOwner& frame) { previous = frame.previous; });
        // Reflex computes this sleep from the previous frame. Let its Present finish first.
        uint64_t joined = 0;
        bool dependency_ready = true;
        bool dependency_known = false;
        uint32_t dependency_failure = 0;
        rsf_native_cpu_reservation current{sizeof(current)};
        if (reservation) {
            const auto joining = now_qpc();
            for (;;) {
                dependency_known = rsf_native_cpu_reservation_read(reservation->session_id, id, &current) != 0;
                if (!dependency_known || current.state != RSF_NATIVE_CPU_RESERVATION_OPEN) {
                    dependency_failure = dependency_known ? 2u : 1u; dependency_ready = false; break;
                }
                if (current.predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_PRESENTED ||
                    current.predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_NONE) break;
                if (current.predecessor_present != RSF_NATIVE_CPU_DEPENDENCY_PENDING) {
                    dependency_failure = 4u;
                    dependency_ready = false; break;
                }
                // A slow predecessor is not an invalid frame. Cancellation is
                // published independently of this callback and ends this wait.
                (void)WaitForSingleObject(completion_event(), 1);
            }
            joined = now_qpc() - joining;
        } else joined = threaded || !host ? 0 : join_previous_present(previous);
        if (!dependency_ready) {
            with_frame(id, [&](CpuOwner& frame) { frame.failed = true; frame.join_ticks = joined; frame.pacing_finished = true; });
            (void)frame_abort(id); trace("reserved source predecessor Present unavailable", id, RSF_BACKEND_ERROR_NOT_READY);
            failure_receipt(FailureStage::PaceDependency, id, reservation->session_id, RSF_BACKEND_ERROR_NOT_READY,
                dependency_failure, 0, nullptr, dependency_known ? &current : nullptr);
            return;
        }
        const auto started = now_qpc();
        uint64_t generation = 0;
        uint32_t mode{}, limit{};
        { const auto requested = options(); mode=requested.reflex_mode; limit=requested.frame_limit_us; }
        uint64_t anti_updates{};
        if (reservation) { auto& owner = anti_lag(); std::lock_guard lock(owner.guard); anti_updates = owner.context.state().updates; }
        const auto pacing=select_cpu_pacing(id,mode,limit);
        bool actual_pacer = pacing == RSF_PACING_REFLEX || pacing == RSF_PACING_XELL;
        if (reservation && pacing == RSF_PACING_ANTILAG2) {
            auto& owner = anti_lag(); std::lock_guard lock(owner.guard);
            const auto state = owner.context.state(); actual_pacer = state.enabled && state.updates > anti_updates;
        }
        bool denied{};{auto& owner=anti_lag();std::lock_guard lock(owner.guard);denied=owner.denied;}
        const auto result = denied ? RSF_BACKEND_ERROR_NOT_READY : rsf_d3d11_present_pace(id,pacing,&generation);
        const auto finished = now_qpc();
        if (!reservation) rsf_native_cpu_paced(id,started,finished);
        trace("sleep at engine pacing boundary", id, result);
        bool withdrawn = false;
        with_frame(id, [&](CpuOwner& frame) {
            frame.pacing_finished = true;
            withdrawn = frame.failed;
            frame.paced = !withdrawn && result == RSF_BACKEND_OK && (!reservation || actual_pacer);
            frame.provider_generation = generation;
            frame.failed |= result != RSF_BACKEND_OK;
            frame.reflex_ticks = finished - started; frame.ready_qpc = finished;
            if (!threaded || reservation) frame.join_ticks = joined;
        });
        if (reservation && (withdrawn || result != RSF_BACKEND_OK)) (void)frame_abort(id);
        if (reservation && (!actual_pacer || denied))
            failure_receipt(FailureStage::PaceSelection, id, reservation->session_id, result,
                uint32_t(pacing) | (uint32_t(denied) << 16) | (uint32_t(actual_pacer) << 17), generation, nullptr, reservation);
        if (result != RSF_BACKEND_OK)
            failure_receipt(FailureStage::PaceCall, id, reservation ? reservation->session_id : 0, result,
                uint32_t(pacing) | (uint32_t(denied) << 16), generation, nullptr, reservation);

}
}
namespace {
void withdraw_reserved_sources(uint64_t session) {
    std::array<uint64_t, 128> withdrawn{};
    {
        std::lock_guard lock(cpu_guard);
        for (size_t i = 0; i < cpu_frames.size(); ++i) {
            auto& frame = cpu_frames[i];
            if (!frame.prospective || frame.session != session || frame.failed) continue;
            rsf_native_cpu_reservation reservation{sizeof(reservation)};
            if (!rsf_native_cpu_reservation_read(session, frame.id, &reservation) ||
                reservation.state == RSF_NATIVE_CPU_RESERVATION_ABORTED || reservation.state == RSF_NATIVE_CPU_RESERVATION_UNBOUND) {
                frame.failed = true;
                // A cancellation may arrive from Main while the worker is inside
                // the vendor sleep. That callback owns the token until it returns.
                if (!frame.render_started && !frame.submitted &&
                    (!frame.pacing_started || frame.pacing_finished)) withdrawn[i] = frame.id;
            }
        }
    }
    for (auto id : withdrawn) if (id) (void)frame_abort(id);
}
}
extern "C" RSF_RUNTIME_API int32_t rsf_native_latency_window_bind(const rsf_native_latency_window* binding) {
    if (!binding || binding->struct_size<sizeof(*binding) || !binding->session_id || !binding->window_key) return RSF_ERROR_INVALID_ARGUMENT;
    if (binding->version!=1) return RSF_ERROR_ABI_MISMATCH;
    DWORD pid{}; const auto thread=GetWindowThreadProcessId(reinterpret_cast<HWND>(uintptr_t(binding->window_key)),&pid);
    if (!thread || pid!=GetCurrentProcessId()) return RSF_ERROR_INVALID_ARGUMENT;
    std::lock_guard lock(cpu_guard);
    if (receipt_window.session_id!=binding->session_id || receipt_window.window_key!=binding->window_key)
        receipt_routes={sizeof(receipt_routes),1};
    receipt_window=*binding; receipt_thread=thread; return RSF_OK;
}
extern "C" RSF_RUNTIME_API int rsf_native_input_routes_get(rsf_native_input_routes* out) {
    if (!out || out->struct_size<sizeof(*out) || out->version!=1) return 0;
    std::lock_guard lock(cpu_guard); *out=receipt_routes; return 1;
}
namespace {
void route_input_receipt(const rsf_game_cpu_event* event) {
    if (event->struct_size<sizeof(rsf_game_cpu_receipt_event) || event->stage!=RSF_GAME_CPU_INPUT_EVENT || event->source_frame_id) return;
    const auto& receipt=*reinterpret_cast<const rsf_game_cpu_receipt_event*>(event);
    if (!event->input_kind && !rsf_native_cpu_is_ping_message(event->message_id)) return;
    const auto generation=rsf_d3d11_present_generation();
    uint64_t selected{};
    {
        std::lock_guard lock(cpu_guard);
        DWORD window_process{};
        const auto window_thread=receipt_window.window_key ?
            GetWindowThreadProcessId(reinterpret_cast<HWND>(uintptr_t(receipt_window.window_key)),&window_process) : 0;
        const bool target=receipt_window.session_id==event->session_id && window_thread &&
            window_thread==receipt_thread && window_process==GetCurrentProcessId() &&
            (receipt.window_key ? receipt.window_key==receipt_window.window_key : GetCurrentThreadId()==receipt_thread);
        std::array<rsf::latency::ReceiptOwner,128> candidates{};
        if (target) for (size_t i=0;i<cpu_frames.size();++i) {
            const auto& owner=cpu_frames[i]; rsf_native_cpu_frame facts{sizeof(facts)};
            const bool eligible=owner.id && owner.session==event->session_id && owner.pacing_finished &&
                !owner.failed && !owner.ended && !owner.render_started && !owner.submitted &&
                rsf_native_cpu_read(owner.session,owner.id,&facts) && !facts.failed && !facts.ended &&
                facts.stage_mask==7u && !(facts.flags&(RSF_GAME_CPU_INPUT_SUBSTITUTE|RSF_GAME_CPU_DERIVED_BOUNDARY));
            candidates[i]={owner.id,owner.session,owner.provider_generation,owner.ready_qpc,
                facts.timestamps_qpc[RSF_GAME_CPU_SIMULATION_BEGIN],eligible};
        }
        selected=rsf::latency::receipt_owner(candidates,event->session_id,generation,event->timestamp_qpc);
        if (!selected) { ++receipt_routes.unmatched; return; }
        if (receipt.window_key) ++receipt_routes.matched_window; else ++receipt_routes.matched_thread;
        receipt_routes.last_receipt=receipt.receipt_id; receipt_routes.last_source=selected;
    }
    // Provider generation and its existing token are checked under their owner
    // locks. A concurrently retired token refuses; no replacement is acquired.
    const auto result=rsf_d3d11_present_receipt(generation,selected,event->input_kind,event->message_id);
    std::lock_guard lock(cpu_guard);
    if (result==RSF_BACKEND_OK) ++receipt_routes.routed; else ++receipt_routes.failed;
}
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_cpu(const rsf_game_cpu_event* event) {
    if (!event || event->struct_size < sizeof(*event)) return;
    // Receipt telemetry is not a frame marker. Token routing is handled separately.
    if (event->event_version==RSF_GAME_CPU_EVENT_VERSION_RECEIPT) { route_input_receipt(event); return; }
    if ((event->flags&(RSF_GAME_CPU_INPUT_SUBSTITUTE|RSF_GAME_CPU_DERIVED_BOUNDARY)) &&
        (event->stage==RSF_GAME_CPU_INPUT_SAMPLE || event->stage==RSF_GAME_CPU_SIMULATION_BEGIN ||
         event->stage==RSF_GAME_CPU_SIMULATION_END)) return;
    if (!rsf_d3d11_present_has_owner()) return;
    const auto id = event->source_frame_id;
    // Only these stages can withdraw/adopt/end reservations. Poll completion,
    // input messages and simulation facts cannot change their lifetime.
    if (!rendered_source.load() && (event->stage == RSF_GAME_CPU_SOURCE_RESERVE ||
        event->stage == RSF_GAME_CPU_SOURCE_ABORT || event->stage == RSF_GAME_CPU_FRAME_BEGIN ||
        event->stage == RSF_GAME_CPU_FRAME_END)) withdraw_reserved_sources(event->session_id);
    if (event->stage >= RSF_GAME_CPU_SOURCE_RESERVE && event->stage <= RSF_GAME_CPU_SOURCE_ABORT) {
        if (rendered_source.load() || event->stage != RSF_GAME_CPU_SOURCE_RESERVE) return;
        rsf_native_cpu_reservation reservation{sizeof(reservation)};
        if (!rsf_native_cpu_reservation_read(event->session_id, id, &reservation) ||
            reservation.state != RSF_NATIVE_CPU_RESERVATION_OPEN) return;
        {
            std::lock_guard lock(cpu_guard);
            auto& frame = cpu_frames[id % cpu_frames.size()];
            if (frame.id == id || (frame.pacing_started && !frame.pacing_finished) ||
                (frame.id && !frame.ended && !(frame.failed && !frame.render_started && !frame.submitted))) return;
            frame = {}; frame.id = id; frame.session = event->session_id; frame.prospective = true;
            frame.previous = reservation.predecessor_source_id;
        }
        const auto acquired = frame_acquire(id);
        bool cancelled = false;
        with_frame(id, [&](CpuOwner& frame) {
            frame.failed |= acquired != RSF_BACKEND_OK;
            cancelled = frame.failed;
        });
        if (cancelled) (void)frame_abort(id);
        else pace_cpu_frame(id, &reservation);
        return;
    }
    if (event->stage != RSF_GAME_CPU_FRAME_BEGIN && event->stage != RSF_GAME_CPU_FRAME_END) {
        bool prospective = false, failed = false;
        with_frame(id, [&](CpuOwner& frame) {
            prospective = frame.prospective;
            if (prospective) {
                frame.failed |= frame.provider_generation != rsf_d3d11_present_generation() ||
                    (event->stage == RSF_GAME_CPU_SIMULATION_BEGIN && (event->flags & RSF_GAME_CPU_INPUT_SUBSTITUTE));
                failed = frame.failed;
            }
        });
        if (prospective) {
            rsf_native_cpu_reservation reservation{sizeof(reservation)};
            if (failed || !rsf_native_cpu_reservation_read(event->session_id, id, &reservation) ||
                reservation.state != RSF_NATIVE_CPU_RESERVATION_BOUND) return;
        }
    }
    if (event->stage == RSF_GAME_CPU_FRAME_BEGIN) {
        const auto joined_at = now_qpc();
        bool reserved = false;
        with_frame(id, [&](const CpuOwner& frame) { reserved = frame.prospective && frame.session == event->session_id; });
        const auto result = reserved ? RSF_BACKEND_OK : frame_acquire(id);
        if (auto* sdk=rsf_d3d11_present_host()) {
            rsf_streamline_latency_status latency{}; latency.struct_size=sizeof(latency);
            if (rsf_streamline_host_latency_status(sdk,&latency)==RSF_BACKEND_OK)
                rsf_native_cpu_ping_message(latency.pcl_message_id);
        }
        const auto ready_at = now_qpc();
        trace("reserve frame before BeginFrame dispatch", id, result);
        const auto previous = last_begun.exchange(id, std::memory_order_relaxed);
        if (reserved) {
            rsf_native_cpu_reservation reservation{sizeof(reservation)};
            const bool adopted = rsf_native_cpu_reservation_read(event->session_id, id, &reservation) &&
                reservation.state == RSF_NATIVE_CPU_RESERVATION_ADOPTED && reservation.poll_begin_qpc;
            uint64_t began{}, ready{}; bool paced = false;
            with_frame(id, [&](CpuOwner& frame) {
                frame.failed |= !adopted; paced = frame.paced && !frame.failed;
                ready = frame.ready_qpc; began = ready - frame.reflex_ticks;
            });
            if (!adopted) (void)frame_abort(id);
            if (paced && began && ready) rsf_native_cpu_paced(id, began, ready);
            return;
        }
        std::lock_guard<std::mutex> lock(cpu_guard);
        auto& frame = cpu_frames[id % cpu_frames.size()];
        // Another id here is at least a full ring old; a frame that never sent FRAME_END is
        // reclaimed too, or this index would refuse every later frame.
        if (frame.id != id || frame.ended) {
            frame = {}; frame.id = id; frame.session = event->session_id; frame.failed = result != RSF_BACKEND_OK;
            frame.join_ticks = joined_at - event->timestamp_qpc;
            frame.ready_qpc = ready_at; frame.previous = previous < id ? previous : 0;
        }
    } else if (rendered_source.load() && event->stage != RSF_GAME_CPU_FRAME_END) {
        // Facts remain in native_cpu; no input markers, inferred sleep or PCL claims.
        return;
    } else if (event->stage == RSF_GAME_CPU_PACING) {
        if (!(event->event_version==RSF_GAME_CPU_EVENT_VERSION && (event->flags & RSF_GAME_CPU_INPUT_KEYED))) pace_cpu_frame(id);
    } else if (event->stage == RSF_GAME_CPU_SIMULATION_BEGIN) {
        if (event->flags & RSF_GAME_CPU_DERIVED_BOUNDARY) ++derived_boundaries;
        bool reserved = false;
        with_frame(id, [&](const CpuOwner& frame) { reserved = frame.prospective && !frame.failed; });
        if (reserved) {
            rsf_native_cpu_reservation reservation{sizeof(reservation)};
            if (!rsf_native_cpu_reservation_read(event->session_id, id, &reservation) ||
                reservation.state != RSF_NATIVE_CPU_RESERVATION_BOUND || !reservation.poll_begin_qpc) return;
        }
        trace("actual simulation begin", id, frame_marker(id, RSF_LATENCY_SIMULATION_START));
    } else if (event->stage == RSF_GAME_CPU_INPUT_SAMPLE) {
        bool reserved = false;
        with_frame(id, [&](const CpuOwner& frame) { reserved = frame.prospective; });
        if (reserved) return; // Neither a second sleep nor simulation-start at the poll.
        if (event->event_version == RSF_GAME_CPU_EVENT_VERSION && (event->flags & RSF_GAME_CPU_INPUT_KEYED) &&
            !(event->flags & RSF_GAME_CPU_INPUT_SUBSTITUTE)) pace_cpu_frame(id);
        // Input sampling is not a simulation-start callback.
    } else if (event->stage == RSF_GAME_CPU_INPUT_EVENT) {
        bool reserved = false;
        with_frame(id, [&](const CpuOwner& frame) { reserved = frame.prospective; });
        if (reserved && (event->flags & (RSF_GAME_CPU_INPUT_UNFRAMED | RSF_GAME_CPU_WINDOW_MESSAGE))) return;
        trace("input received", id, rsf_d3d11_present_input(id, event->input_kind, event->message_id));
    } else if (event->stage == RSF_GAME_CPU_SIMULATION_END) {
        if (event->flags & RSF_GAME_CPU_DERIVED_BOUNDARY) ++derived_boundaries;
        trace("simulation end", id, frame_marker(id, RSF_LATENCY_SIMULATION_END));
    } else if (event->stage == RSF_GAME_CPU_FRAME_END) {
        rsf_native_cpu_frame cpu{}; cpu.struct_size = sizeof(cpu);
        bool prospective_failed = false;
        with_frame(id, [&](const CpuOwner& frame) { prospective_failed = frame.prospective && frame.failed; });
        const bool aborted = prospective_failed || (event->flags & RSF_GAME_CPU_RENDER_SKIPPED) ||
            (rsf_native_cpu_read(event->session_id, id, &cpu) && (rendered_source.load() ? (cpu.stage_mask & 9u) != 9u : (cpu.stage_mask & 15u) != 15u));
        if (aborted) frame_abort(id);
        with_frame(id, [&](CpuOwner& frame) {
            frame.ended = true; frame.expected_count = event->render_expected;
            if (aborted) frame.failed = true;
        });
        if (aborted) complete_frame(id);
        audit_frame(id);
        rsf_native_cpu_counters totals{}; totals.struct_size=sizeof(totals);
        static rsf_native_cpu_counters reported{};
        static rsf::latency::AntiLagState reported_anti{};
        if (rsf_native_cpu_counters_get(&totals) && (totals.frames<reported.frames || totals.frames-reported.frames >= 120)) {
            if (totals.frames<reported.frames) reported={};
            auto& owner=anti_lag(); rsf::latency::AntiLagState anti{};
            { std::lock_guard lock(owner.guard); anti=owner.context.state(); }
            const auto delta=[](uint64_t a,uint64_t b) { return static_cast<unsigned long long>(a-b); };
            char text[384];
            std::snprintf(text,sizeof(text),"Latency window: inputs_keyboard=%llu mouse=%llu controller=%llu pcl_ping=%llu "
                "substitute=%llu duplicate=%llu marker_order_violations=%llu pcl_expired=%llu Anti-Lag2_updates=%llu Anti-Lag2_refused=%llu",
                delta(totals.input_events[0],reported.input_events[0]),delta(totals.input_events[1],reported.input_events[1]),
                delta(totals.input_events[2],reported.input_events[2]),delta(totals.pings,reported.pings),
                delta(totals.substitutes,reported.substitutes),delta(totals.duplicates,reported.duplicates),
                delta(totals.order_violations,reported.order_violations),delta(totals.expired_messages,reported.expired_messages),
                delta(anti.updates,reported_anti.updates),delta(anti.refused,reported_anti.refused));
            current_sink(false)(text); reported=totals; reported_anti=anti;
        }
    }
}
namespace {
// RenderSubmitEnd, once per frame. Ordered frames send it before PresentStart; the threaded
// order keeps UE's EndFrame position, after Present.
void end_render_marker(uint64_t id, const char* stage) {
    bool end = false;
    uint64_t session{};
    with_frame(id, [&](CpuOwner& frame) {
        if (!frame.failed && frame.render_started && !frame.submit_ended && !frame.submit_ending)
            { session = frame.session; end = frame.submit_ending = true; }
    });
    if (!end) return;
    rsf_native_cpu_frame facts{}; facts.struct_size = sizeof(facts);
    const bool known = rsf_native_cpu_read(session,id,&facts) != 0;
    const uint32_t required = facts.flags & RSF_GAME_CPU_INPUT_KEYED ? 7u : 4u;
    if (!rendered_source.load() && (!known || facts.failed || (facts.marker_mask & required) != required)) {
        with_frame(id, [](CpuOwner& frame) { frame.submit_ending = false; }); return;
    }
    bool marked = false;
    const auto result = frame_marker(id, RSF_LATENCY_RENDER_SUBMIT_END, &marked);
    with_frame(id, [&](CpuOwner& frame) {
        frame.submit_ending = false;
        frame.submit_ended = marked; frame.failed |= result != RSF_BACKEND_OK;
    });
    trace(stage, id, result);
}
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_frame(const rsf_game_render_pass* pass, uint32_t begin) {
    if (!pass || pass->role != RSF_GAME_RENDER_FRAME || !pass->source_frame_id) return;
    auto* host = rsf_d3d11_present_host(); if (!rsf_d3d11_present_has_owner()) return;
    with_frame(pass->source_frame_id, [&](CpuOwner& frame) {
        if (begin) {
            ++frame.rhi_begins;
            if (rendered_source.load()) {
                if (frame.rhi_begins != 1 || frame.session != pass->session_id) frame.failed = true;
                frame.source_qpc = now_qpc();
                LARGE_INTEGER frequency{}; QueryPerformanceFrequency(&frequency);
                frame.source_frequency = uint64_t(frequency.QuadPart);
                static uint64_t last_qpc{}, last_session{}, last_native{};
                if (last_session == pass->session_id && pass->native_frame == last_native + 1 && frame.source_qpc > last_qpc)
                    frame.source_delta_ms = float(double(frame.source_qpc - last_qpc) * 1000.0 / frame.source_frequency);
                last_qpc = frame.source_qpc; last_session = pass->session_id; last_native = pass->native_frame;
                frame.provider_generation = rsf_d3d11_present_generation();
            }
        } else ++frame.rhi_ends;
    });
    // BeginFrame is queued before CPU sleep/input. On the D3D12 proxy device it contains no
    // rendering yet; a driver marker here measures that empty-queue wait as GPU frame work.
    // Keep its copied identity, but open the vendor interval at actual scene submission.
    if (begin) { rhi_frame_source = pass->source_frame_id; return; }
    // Presented frames closed render submission before PresentStart. This covers the rest.
    end_render_marker(pass->source_frame_id, "full RHI frame end");
    // A completed native frame without Present cannot be reused as a later frame's token.
    if (frame_live(host, pass->source_frame_id)) frame_abort(pass->source_frame_id);
    complete_frame(pass->source_frame_id);
    if (rhi_frame_source == pass->source_frame_id) rhi_frame_source = 0;
    audit_frame(pass->source_frame_id);
}
namespace {
void start_render_marker(uint64_t id) {
    bool start = false;
    uint64_t session{}, provider_generation{};
    bool prospective = false, post_update = false, owner_failed = false;
    with_frame(id, [&](CpuOwner& frame) {
        if (frame.failed) { owner_failed = true; session = frame.session; return; }
        if (frame.render_started || frame.render_starting) return;
        if (rendered_source.load()) { frame.render_started = true; return; }
        start = frame.render_starting = true;
        session = frame.session; prospective = frame.prospective;
        post_update = frame.submitted && frame.scene_begins && frame.first_submission && frame.first_view &&
            frame.provider_generation == (provider_generation = rsf_d3d11_present_generation());
    });
    if (!start) {
        if (owner_failed) failure_receipt(FailureStage::RenderOwner,id,session,RSF_BACKEND_ERROR_NOT_READY,1);
        return;
    }
    // Reserve only the attempt across callbacks. No end marker is authorized until the
    // CPU ledger and provider have both accepted the actual submission start.
    const auto finish = [&](bool accepted, bool failed = false) {
        with_frame(id, [&](CpuOwner& frame) {
            frame.render_starting = false; frame.render_started = accepted; frame.failed |= failed;
        });
    };
    rsf_native_cpu_frame facts{}; facts.struct_size = sizeof(facts);
    const bool cpu_known = rsf_native_cpu_read(session, id, &facts) != 0;
    if (!cpu_known || facts.failed) {
        finish(false); failure_receipt(FailureStage::RenderCpu,id,session,RSF_BACKEND_ERROR_NOT_READY,0,provider_generation,cpu_known ? &facts : nullptr); return;
    }
    rsf_native_cpu_reservation reservation{sizeof(reservation)};
    const bool reservation_known = prospective && rsf_native_cpu_reservation_read(session,id,&reservation);
    const bool bound = reservation_known &&
        reservation.state == RSF_NATIVE_CPU_RESERVATION_BOUND && reservation.poll_begin_qpc;
    if (prospective && (!bound || !post_update || (facts.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE) ||
        (facts.stage_mask & 7u) != 7u)) {
        finish(false, true);
        failure_receipt(FailureStage::RenderProspective,id,session,RSF_BACKEND_ERROR_NOT_READY,
            uint32_t(!bound) | (uint32_t(!post_update) << 1) | (uint32_t((facts.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE) != 0) << 2) |
            (uint32_t((facts.stage_mask & 7u) != 7u) << 3),provider_generation,&facts,reservation_known ? &reservation : nullptr); return;
    }
    // Unkeyed FRAME_BEGIN/END fallback has no simulation interval to close or submit.
    // Only observed input and simulation callbacks authorize vendor intervals.
    if ((facts.stage_mask & 15u)!=15u || (facts.marker_mask & 3u)!=3u ||
        (facts.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE)) {
        finish(false); failure_receipt(FailureStage::RenderInput,id,session,RSF_BACKEND_ERROR_NOT_READY,0,provider_generation,&facts); return;
    }
    facts.struct_size = sizeof(facts);
    const bool recheck_known = rsf_native_cpu_read(session,id,&facts) != 0;
    if (!recheck_known || facts.failed) {
        finish(false); failure_receipt(FailureStage::RenderRecheck,id,session,RSF_BACKEND_ERROR_NOT_READY,0,provider_generation,recheck_known ? &facts : nullptr); return;
    }
    const uint32_t required = facts.flags & RSF_GAME_CPU_INPUT_KEYED ? 3u : 1u;
    if ((facts.marker_mask & required) != required) {
        finish(false); failure_receipt(FailureStage::RenderMarkers,id,session,RSF_BACKEND_ERROR_NOT_READY,required,provider_generation,&facts); return;
    }
    bool marked = false;
    const auto result = frame_marker(id, RSF_LATENCY_RENDER_SUBMIT_START, &marked);
    finish(marked, result != RSF_BACKEND_OK);
    trace("actual graphics submit begin", id, result);
    if (result != RSF_BACKEND_OK || !marked)
        failure_receipt(FailureStage::RenderProvider,id,session,result,uint32_t(marked),provider_generation,&facts,reservation_known ? &reservation : nullptr);
}
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_graphics(uint32_t api) { if (api==11 || api==12) graphics_api.store(api); }
extern "C" RSF_RUNTIME_API void rsf_native_latency_render(uint64_t id,uint32_t begin) {
    if (begin) start_render_marker(id); else end_render_marker(id,"render submission end");
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_present(uint64_t id,void* chain,uint32_t begin) {
    if (!id) return;
    if (begin) {
        start_render_marker(id);
        if (!threaded_reflex.load()) end_render_marker(id,"render submission end before present");
        bool start = false;
        uint64_t session{};
        with_frame(id, [&](CpuOwner& frame) {
            if (frame.failed || !frame.render_started || frame.present_started || frame.present_starting) return;
            session = frame.session; start = frame.present_starting = true;
        });
        if (!start) return;
        rsf_native_cpu_frame facts{}; facts.struct_size = sizeof(facts);
        const bool rendered = rendered_source.load() != 0;
        const bool known = rsf_native_cpu_read(session,id,&facts) != 0;
        const bool ordered = (facts.flags & RSF_GAME_CPU_INPUT_KEYED) || !threaded_reflex.load();
        bool submit_ended = false;
        with_frame(id, [&](const CpuOwner& frame) { submit_ended = frame.submit_ended; });
        const uint32_t required = facts.flags & RSF_GAME_CPU_INPUT_KEYED ? 15u : 4u;
        if ((ordered && !submit_ended) || (!rendered && (!known || facts.failed || (facts.marker_mask & required) != required))) {
            with_frame(id, [](CpuOwner& frame) { frame.present_starting = false; }); return;
        }
        if (!rendered) anti_lag_present(id,chain);
        bool marked = false;
        const auto result = frame_marker(id,RSF_LATENCY_PRESENT_START,&marked);
        with_frame(id, [&](CpuOwner& frame) {
            frame.present_starting = false; frame.present_started = marked; frame.failed |= result != RSF_BACKEND_OK;
        });
        trace("present begin",id,result);
    } else {
        bool end = false;
        with_frame(id, [&](CpuOwner& frame) {
            if (!frame.failed && frame.present_started && !frame.present_ended && !frame.present_ending)
                end = frame.present_ending = true;
        });
        if (end) {
            bool marked = false;
            const auto result = frame_marker(id,RSF_LATENCY_PRESENT_END,&marked);
            with_frame(id, [&](CpuOwner& frame) {
                frame.present_ending = false; frame.present_ended = marked; frame.failed |= result != RSF_BACKEND_OK;
            });
            trace("present end",id,result);
        }
        complete_frame(id);
    }
}
extern "C" RSF_RUNTIME_API int rsf_native_latency_frame_get(rsf_native_latency_frame* out) {
    if (!out || out->struct_size<sizeof(*out) || out->version!=1 || !out->source) return 0;
    return with_frame(out->source,[&](const CpuOwner& f) {
        out->generation=f.provider_generation; out->ready_qpc=f.ready_qpc; out->paced=f.paced;
        out->failed=f.failed; out->completed=presented_through.load()>=f.id;
    });
}
extern "C" RSF_RUNTIME_API int rsf_native_latency_status_get(rsf_native_latency_status* out) {
    if (!out || out->struct_size<sizeof(*out) || out->version!=1) return 0;
    *out={sizeof(*out),1};
    if (rendered_source.load()) return 1; // Physical input timing and low latency unavailable.
    rsf_fg_status status{}; status.struct_size=sizeof(status); anti_lag_status(status);
    out->owner=status.pacing_owner; out->available=status.low_latency_available;
    out->effective_mode=status.effective_reflex; out->paced_frames=status.latency_paced_frames;
    rsf_streamline_latency_status sdk{}; sdk.struct_size=sizeof(sdk);
    if (auto* host=rsf_d3d11_present_host(); host && rsf_streamline_host_latency_status(host,&sdk)==RSF_BACKEND_OK) {
        out->pcl_available=sdk.pcl_message_id>=0xc000; out->report_available=sdk.latency_report_available;
        out->marker_calls=sdk.marker_calls;
    }
    out->events.struct_size=sizeof(out->events); (void)rsf_native_cpu_counters_get(&out->events);
    out->derived_boundaries=derived_boundaries.load()+out->events.derived_frame_ends; return 1;
}
extern "C" RSF_RUNTIME_API int rsf_native_latency_ping(void* window) {
    rsf_streamline_latency_status sdk{}; sdk.struct_size=sizeof(sdk);
    auto* host=rsf_d3d11_present_host();
    return window && host && rsf_streamline_host_latency_status(host,&sdk)==RSF_BACKEND_OK &&
        sdk.pcl_message_id>=0xc000 && PostMessageW(static_cast<HWND>(window),sdk.pcl_message_id,0,0);
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_retire(uint64_t session,uint64_t through) {
    // These tokens have no submitted GPU work. Prepared frames remain owned until actual presentation.
    if (auto* host=rsf_d3d11_present_host()) (void)rsf_streamline_host_retire_through(host,through);
    std::lock_guard lock(cpu_guard);
    for (auto& frame:cpu_frames) if (frame.session==session && frame.id && frame.id<=through && !frame.render_started) {
        frame.failed=true;
    }
    complete_frame(through);
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_end_session(uint64_t session) {
    uint64_t last=last_begun.load(); rsf_native_cpu_frame facts{};facts.struct_size=sizeof(facts);
    if (last && !rsf_native_cpu_read(session,last,&facts)) return;
    if (auto* host=rsf_d3d11_present_host()) {
        (void)rsf_streamline_host_retire_through(host,UINT64_MAX);
        (void)rsf_streamline_host_reset_sources(host);
        (void)rsf_streamline_host_reflex(host,RSF_REFLEX_OFF,0);
    }
    {std::lock_guard lock(cpu_guard);cpu_frames={};receipt_window={};receipt_thread=0;receipt_routes={sizeof(receipt_routes),1};}
    {auto& owner=anti_lag();std::lock_guard lock(owner.guard);owner.context.reset_sources();owner.pacing=RSF_PACING_NONE;owner.reflex_configured=false;}
    last_begun=0;presented_through=0;join_skip=0;derived_boundaries=0;
    rsf_native_cpu_release();
    rsf_native_latency_release_claim(1);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_submission(const rsf_game_render_pass* pass, uint32_t begin) {
    if (!pass || !pass->source_frame_id || !(pass->flags & RSF_GAME_RENDER_PRIMARY) ||
        !(pass->flags & RSF_GAME_RENDER_AFTER_SIMULATION) || !rsf_d3d11_present_has_owner()) return;
    const auto id = pass->source_frame_id;
    watch_submission(id, pass->screen, begin != 0);
    with_frame(id, [&](CpuOwner& frame) {
        if (!begin) { ++frame.scene_ends; return; }
        if (!frame.scene_begins) { frame.first_submission = pass->submission_id; frame.first_view = pass->view_key; }
        ++frame.scene_begins; frame.last_submission = pass->submission_id; frame.last_view = pass->view_key;
        frame.screen = pass->screen; frame.native_frame = uint32_t(pass->native_frame);
    });
    if (begin) {
        // Dropped older frames have finished CPU ownership and never began RHI submission.
        // Queued commands are FIFO here. Release only those SDK tokens, never an active renderer.
        std::array<uint64_t, 128> dropped{};
        {
            std::lock_guard<std::mutex> lock(cpu_guard);
            for (size_t i = 0; i < cpu_frames.size(); ++i) {
                auto& frame = cpu_frames[i];
                if (frame.id && frame.id < id && frame.ended && !frame.submitted) { dropped[i] = frame.id; frame.failed = true; }
            }
            auto& frame = cpu_frames[id % cpu_frames.size()];
            if (frame.id != id || frame.failed || frame.submitted) return;
            frame.submitted = true;
            frame.submit_begin = now_qpc();
        }
        for (auto old : dropped) if (old) frame_abort(old);
        start_render_marker(id);
    } else {
        with_frame(id, [](CpuOwner& frame) { frame.submit_end = now_qpc(); frame.render_complete = true; });
    }
}
// CPU ledger leaves for the graphics owner in native_fg.rs (rsf_nfg_platform cpu_* entries).
extern "C" uint32_t rsf_nfg_cpu_rendered_source(void) { return rendered_source.load(); }
extern "C" void rsf_nfg_cpu_owner_read(uint64_t source, rsf_nfg_cpu_owner* out) {
    std::lock_guard<std::mutex> lock(cpu_guard);
    const auto& frame = cpu_frames[source % cpu_frames.size()];
    *out = {frame.id, frame.session, frame.provider_generation, frame.ready_qpc, frame.source_qpc, frame.source_frequency,
        frame.first_submission, frame.first_view, frame.join_ticks, frame.reflex_ticks, frame.submit_begin, frame.submit_end,
        frame.submitted, frame.render_complete, frame.paced, frame.prospective, frame.failed,
        frame.rhi_begins, frame.scene_begins, frame.scene_ends, frame.source_delta_ms};
}
extern "C" void rsf_nfg_cpu_count(uint64_t source, uint32_t counter) {
    with_frame(source, [&](CpuOwner& frame) {
        if (counter == RSF_NFG_COUNT_SR_CALL) ++frame.sr_calls;
        else if (counter == RSF_NFG_COUNT_WINDOW) ++frame.windows;
    });
}
// Present thread. The RHI frame open on this thread counts a Present that matched no window.
extern "C" void rsf_nfg_cpu_present_observed(uint64_t presenting) {
    const auto counted = presenting ? presenting : rhi_frame_source;
    if (counted) with_frame(counted, [&](CpuOwner& frame) {
        ++frame.presents;
        if (!presenting || presenting != rhi_frame_source) ++frame.mismatched_window;
    });
}
extern "C" void rsf_nfg_cpu_progress(uint64_t source, uint32_t* live, uint32_t* presented) {
    auto* host = rsf_d3d11_present_host();
    *live = frame_live(host, source);
    *presented = *live && frame_presented(host, source);
}
extern "C" void rsf_nfg_cpu_fail(uint64_t source) { with_frame(source, [](CpuOwner& frame) { frame.failed = true; }); }
extern "C" void rsf_nfg_cpu_complete(uint64_t source) { complete_frame(source); }
extern "C" void rsf_nfg_cpu_anti_lag_status(rsf_fg_status* status) { anti_lag_status(*status); }
extern "C" void rsf_nfg_cpu_stage(const char* stage, uint64_t source, uint64_t native_frame, int32_t result,
    const rsf_nfg_cpu_owner* owner, const rsf_game_render_pass* window) {
    log_fg_stage(stage, source, native_frame, result, owner, window);
}
extern "C" void rsf_nfg_cpu_trace(const char* stage, uint64_t source, int32_t result) { trace(stage, source, result); }
extern "C" void rsf_nfg_cpu_failure(uint32_t stage, uint64_t source, uint64_t session, int32_t result, uint32_t detail) {
    static constexpr FailureStage stages[]{FailureStage::SrPrerequisite, FailureStage::SrToken,
        FailureStage::SrInitialize, FailureStage::SrSharedEvaluate};
    static_assert(RSF_NFG_FAILURE_SR_PREREQUISITE == 0 && RSF_NFG_FAILURE_SR_TOKEN == 1 &&
        RSF_NFG_FAILURE_SR_INITIALIZE == 2 && RSF_NFG_FAILURE_SR_SHARED_EVALUATE == 3);
    if (stage < std::size(stages)) failure_receipt(stages[stage], source, session, result, detail);
}
extern "C" void rsf_nfg_cpu_report_timing(const rsf_native_cpu_frame* cpu, const rsf_nfg_cpu_owner* owner,
    uint32_t screen, uint64_t wait_ticks, uint64_t evaluate_ticks) {
    report_frame_timing(*cpu, *owner, screen, wait_ticks, evaluate_ticks);
}