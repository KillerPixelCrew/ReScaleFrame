// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_scene.h>
#include <rescaleframe/native_window.h>
#include <rescaleframe/shared_surface.h>
#include "../../backends/common/d3d12_helpers.h"
#include <d3d11_4.h>
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
#include <limits>
#include <cmath>

namespace {
using Microsoft::WRL::ComPtr;
std::mutex settings_guard;
rsf_native_fg_options requested{sizeof(requested), RSF_FG_OFF, 1, RSF_REFLEX_OFF, 0, 0};
rsf_native_fg_status cached{};
rsf_backend_log_fn logger = nullptr;
void* log_user = nullptr;
uint32_t debug_lines = 0;
uint64_t last_error_qpc = 0;
uint32_t last_eligibility = UINT32_MAX, eligibility_lines = 0;
// Whether the vendor was generating at the previous SR evaluation; SR history restarts only when that changes.
bool was_generating = false;
struct Sink {
    rsf_backend_log_fn fn = nullptr; void* user = nullptr;
    explicit operator bool() const { return fn != nullptr; }
    void operator()(const char* text) const { if (fn) fn(user, text); }
};
// The logger, read under its lock. debug_only returns nothing unless debug output was requested.
Sink current_sink(bool debug_only) {
    std::lock_guard<std::mutex> lock(settings_guard);
    return debug_only && !requested.debug ? Sink{} : Sink{logger, log_user};
}
void trace(const char* stage, uint64_t id, int32_t result) {
    rsf_backend_log_fn sink = nullptr; void* user = nullptr;
    { std::lock_guard<std::mutex> lock(settings_guard);
      bool error_due = false;
      if (result != RSF_BACKEND_OK) {
          LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
          error_due = uint64_t(now.QuadPart) - last_error_qpc >= uint64_t(frequency.QuadPart);
          if (error_due) last_error_qpc = uint64_t(now.QuadPart);
      }
      if (requested.debug && (debug_lines++ < 256 || error_due)) { sink = logger; user = log_user; } }
    if (sink) { char text[160]; std::snprintf(text, sizeof(text), "FG/Reflex %s source=%llu result=%d", stage,
        static_cast<unsigned long long>(id), result); sink(user, text); }
}
thread_local rsf_game_render_pass scene{};
// The game's screen class decides, through the one rule every generation consumer applies.
bool generates_on(uint32_t screen) {
    rsf_frame_record record{}; record.struct_size = sizeof(record); record.screen = screen;
    return rsf_frame_allows_fg(&record) != 0;
}
thread_local uint64_t presenting_id = 0;
thread_local uint64_t rhi_frame_source = 0;
// Default order: the game thread sleeps once at its pacing boundary, after the presenting
// thread finished the previous frame's Present and before this frame's input. Nonzero keeps
// UE's one-frame overlap instead, where that sleep races the previous submit/Present markers.
std::atomic<uint32_t> threaded_reflex{0};
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
rsf_backend_result frame_acquire(uint64_t id) {
    return id ? rsf_d3d11_present_acquire(id) : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
}
rsf_backend_result frame_marker(uint64_t id, rsf_latency_marker marker) {
    return rsf_d3d11_present_marker(rsf_d3d11_present_generation(), id, marker, 0);
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
    { std::lock_guard<std::mutex> lock(settings_guard); if (!requested.debug) return; }
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
    uint64_t provider_generation = 0;
    uint64_t id = 0;
    bool submitted = false, ended = false, failed = false, render_complete = false;
    bool paced = false;
    bool render_started = false;
    bool audited = false;
    uint32_t expected_count = 0, scene_begins = 0, scene_ends = 0, sr_calls = 0, presents = 0, windows = 0;
    uint32_t rhi_begins = 0, rhi_ends = 0, screen = 0, native_frame = 0, mismatched_window = 0;
    uint64_t first_submission = 0, last_submission = 0, first_view = 0, last_view = 0;
    uint64_t join_ticks = 0, reflex_ticks = 0, ready_qpc = 0, submit_begin = 0, submit_end = 0;
    uint64_t previous = 0;
    bool submit_ended = false;
};
std::mutex cpu_guard;
std::array<CpuOwner, 128> cpu_frames{};
// Runs f on the ring entry for id under cpu_guard. False when the entry holds another frame.
template<class F> bool with_frame(uint64_t id, F&& f) {
    std::lock_guard<std::mutex> lock(cpu_guard);
    auto& frame = cpu_frames[id % cpu_frames.size()];
    if (frame.id != id) return false;
    f(frame); return true;
}
struct OwnershipTotals {
    uint64_t epoch = 0, frames = 0, expected = 0, scenes = 0, ends = 0, sr = 0, presents = 0, windows = 0;
    uint64_t unpresented = 0, multi = 0, mismatched = 0, native_gaps = 0, rhi_begin = 0, rhi_end = 0;
    uint32_t previous_native = 0;
};
std::array<OwnershipTotals, 8> ownership{};
void audit_frame(uint64_t id) {
    char text[600]{};
    {
        std::lock_guard<std::mutex> lock(cpu_guard);
        auto& frame = cpu_frames[id % cpu_frames.size()];
        if (frame.id != id || frame.audited || !frame.ended || !frame.rhi_ends) return;
        frame.audited = true;
        auto& total = ownership[std::min(frame.screen, 7u)];
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
struct Transfer {
    ComPtr<ID3D11Device> device11;
    ComPtr<ID3D11DeviceContext4> context11;
    rsf_streamline_graphics graphics{};
    rsf_shared_fence* fence = nullptr;
    HANDLE event = nullptr;
    uint64_t serial = 0;
    uint64_t previous_input_qpc = 0;
    // Guards each slot's vendor_* fields and read_state: retirement writes them on the present
    // thread, resources() reads them on the render thread.
    std::mutex vendor_guard;
    struct Slot {
        uint64_t source = 0, complete = 0, vendor_value = 0;
        ComPtr<ID3D12Fence> vendor_fence;
        // Retirement failed, so nothing says when the vendor stopped reading. Held until a later
        // retirement covers it or the provider generation changes (a drained transition).
        bool vendor_unknown = false;
        uint64_t vendor_generation = 0;
        // That failed retirement never ran the restore, so D3D12 still holds depth, motion and
        // HUD-less colour in the read state. resources() returns them to COMMON before reuse.
        bool read_state = false;
        ComPtr<ID3D12CommandAllocator> allocator;
        ComPtr<ID3D12GraphicsCommandList> list;
        ComPtr<ID3D12CommandAllocator> retirement_allocator;
        ComPtr<ID3D12GraphicsCommandList> retirement_list;
        // Prepare moved depth, motion and HUD-less colour to the read state; retire restores them.
        bool restore = false;
        // SR-only storage (surfaces 0, 3 and 4 plus output) exists only once SR evaluated here.
        ComPtr<ID3D12Resource> output;
        rsf_shared_surface* surfaces[5]{};
        rsf_shared_surface* hudless = nullptr;
        DXGI_FORMAT hudless_format = DXGI_FORMAT_UNKNOWN;
        uint32_t hudless_width = 0, hudless_height = 0;
        bool hudless_ready = false;
        uint32_t width = 0, height = 0, output_width = 0, output_height = 0, generation = 0;
        DXGI_FORMAT motion_format = DXGI_FORMAT_UNKNOWN;
        // Optional translucency hints, each shaped like its own source.
        rsf_shared_surface* hints[5]{};
        D3D11_TEXTURE2D_DESC hint_desc[5]{};
        rsf_game_render_pass pass{};
        bool prepared = false;
        uint64_t wait_ticks = 0, evaluate_ticks = 0;
    } slots[6];
};
thread_local Transfer::Slot* tagged_slot = nullptr;
// Process-owned because CPU/vendor callbacks and presentation hooks remain installed. Explicit
// ownership transitions are cold-start only; no shutdown callback frees in-flight inputs.
Transfer& transfer() { static auto* value = new Transfer; return *value; }
Transfer::Slot& slot_for(Transfer& value, uint64_t id) { return value.slots[id % std::size(value.slots)]; }
// The slot already holds inputs for exactly this pass: same source frame, session, view,
// submission and native frame, the identity prepare checks against the presented scene.
bool holds(const Transfer::Slot& slot, const rsf_game_render_pass& pass) {
    return slot.source == pass.source_frame_id && slot.pass.session_id == pass.session_id &&
        slot.pass.view_key == pass.view_key && slot.pass.submission_id == pass.submission_id &&
        slot.pass.native_frame == pass.native_frame;
}
ID3D11Fence* fence11(const Transfer& value) { return static_cast<ID3D11Fence*>(rsf_shared_fence_d3d11(value.fence)); }
ID3D12Fence* fence12(const Transfer& value) { return static_cast<ID3D12Fence*>(rsf_shared_fence_d3d12(value.fence)); }
ID3D11Resource* surface11(rsf_shared_surface* surface) { return static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(surface)); }
ID3D12Resource* surface12(rsf_shared_surface* surface) { return static_cast<ID3D12Resource*>(rsf_shared_surface_d3d12(surface)); }
// Queues the next serial on the D3D11 side. Unflushed: the presentation bridge's own Signal and
// Flush later in the frame submit it, ahead of any CPU wait for slot reuse.
bool signal11(Transfer& value, uint64_t& serial) {
    serial = ++value.serial;
    return SUCCEEDED(value.context11->Signal(fence11(value), serial));
}
bool initialize(Transfer& value, void* context) {
    if (value.fence) return true;
    value.graphics.struct_size = sizeof(value.graphics);
    if (!rsf_d3d11_present_graphics(&value.graphics)) return false;
    auto* native = static_cast<ID3D11DeviceContext*>(context);
    native->GetDevice(&value.device11);
    if (FAILED(native->QueryInterface(IID_PPV_ARGS(&value.context11)))) return false;
    value.event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    return value.event && rsf_shared_fence_create(value.device11.Get(), value.graphics.native_device,
        nullptr, nullptr, &value.fence) == RSF_SHARED_OK;
}
bool create_surface(Transfer& value, rsf_shared_surface*& surface, uint32_t width, uint32_t height, DXGI_FORMAT format) {
    rsf_shared_surface_destroy(surface); surface = nullptr;
    rsf_shared_surface_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_SHARED_SURFACE_ABI_VERSION;
    setup.width = width; setup.height = height; setup.format = format;
    return rsf_shared_surface_create(value.device11.Get(), value.graphics.native_device, &setup, &surface) == RSF_SHARED_OK;
}
// Returns surfaces a failed retirement left in the read state to COMMON on the interop queue,
// ahead of any D3D11 write. The vendor's reads have been joined or drained by now.
bool restore_read_state(Transfer& value, Transfer::Slot& slot, ID3D12Device* device) {
    // A list left open by a failed Close cannot be reset; replace the pair instead.
    if (FAILED(slot.retirement_allocator->Reset()) || FAILED(slot.retirement_list->Reset(slot.retirement_allocator.Get(), nullptr))) {
        slot.retirement_list.Reset(); slot.retirement_allocator.Reset();
        if (!rsf::create_command_list(device, D3D12_COMMAND_LIST_TYPE_DIRECT, &slot.retirement_allocator, &slot.retirement_list) ||
            FAILED(slot.retirement_allocator->Reset()) || FAILED(slot.retirement_list->Reset(slot.retirement_allocator.Get(), nullptr)))
            return false;
    }
    for (auto* surface : {slot.surfaces[1], slot.surfaces[2], slot.hudless}) if (surface)
        rsf::transition(slot.retirement_list.Get(), surface12(surface), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
    if (FAILED(slot.retirement_list->Close())) return false;
    auto* queue = static_cast<ID3D12CommandQueue*>(value.graphics.queue);
    ID3D12CommandList* lists[]{slot.retirement_list.Get()}; queue->ExecuteCommandLists(1, lists);
    // Waited on the CPU: rare, and the caller may destroy these surfaces right after.
    slot.complete = ++value.serial;
    return SUCCEEDED(queue->Signal(fence12(value), slot.complete)) &&
        SUCCEEDED(rsf::wait_fence(fence12(value), slot.complete, value.event));
}
// Joins the slot's previous use and sizes its storage. Generation keeps only depth and motion
// (surfaces 1 and 2) per slot; reconstruct adds what SR evaluation needs on top.
bool resources(Transfer& value, Transfer::Slot& slot, const rsf_dlss_frame& frame, bool reconstruct) {
    ComPtr<ID3D12Fence> vendor_fence; uint64_t vendor_value = 0; bool read_state = false;
    {
        std::lock_guard<std::mutex> lock(value.vendor_guard);
        if (slot.vendor_unknown) {
            if (slot.vendor_generation == rsf_d3d11_present_generation()) return false;
            // A drained provider switch ended every read of the old provider.
            slot.vendor_unknown = false;
        }
        vendor_fence = slot.vendor_fence; vendor_value = slot.vendor_value; read_state = slot.read_state;
    }
    const auto started = now_qpc();
    // D3D11 signals are queued unflushed. Submit them before blocking on one, which happens when
    // the same frame reuses the slot, e.g. capture after a failed SR evaluation.
    if (slot.complete && fence12(value)->GetCompletedValue() < slot.complete) value.context11->Flush();
    if (FAILED(rsf::wait_fence(fence12(value), slot.complete, value.event)) ||
        FAILED(rsf::wait_fence(vendor_fence.Get(), vendor_value, value.event))) return false;
    slot.wait_ticks = now_qpc() - started;
    auto* device = static_cast<ID3D12Device*>(value.graphics.device);
    if (!slot.allocator && !rsf::create_command_list(device, D3D12_COMMAND_LIST_TYPE_DIRECT, &slot.allocator, &slot.list))
        return false;
    if (!slot.retirement_allocator && !rsf::create_command_list(device, D3D12_COMMAND_LIST_TYPE_DIRECT,
            &slot.retirement_allocator, &slot.retirement_list)) return false;
    if (read_state) {
        if (!restore_read_state(value, slot, device)) return false;
        std::lock_guard<std::mutex> lock(value.vendor_guard); slot.read_state = false;
    }
    // Motion keeps its producer's precision: the transfer is a whole-resource copy.
    if (!frame.motion) return false;
    D3D11_TEXTURE2D_DESC motion{}; static_cast<ID3D11Texture2D*>(frame.motion)->GetDesc(&motion);
    if (motion.Format != DXGI_FORMAT_R16G16_FLOAT && motion.Format != DXGI_FORMAT_R32G32_FLOAT) return false;
    if (slot.width != frame.render_width || slot.height != frame.render_height || slot.motion_format != motion.Format ||
        slot.output_width != frame.output_width || slot.output_height != frame.output_height) {
        for (auto*& surface : slot.surfaces) { rsf_shared_surface_destroy(surface); surface = nullptr; }
        for (auto*& surface : slot.hints) { rsf_shared_surface_destroy(surface); surface = nullptr; }
        slot.output.Reset(); slot.width = slot.height = 0;
        if (!create_surface(value, slot.surfaces[1], frame.render_width, frame.render_height, DXGI_FORMAT_R32_FLOAT) ||
            !create_surface(value, slot.surfaces[2], frame.render_width, frame.render_height, motion.Format)) return false;
        slot.width = frame.render_width; slot.height = frame.render_height; slot.motion_format = motion.Format;
        slot.output_width = frame.output_width; slot.output_height = frame.output_height; ++slot.generation;
    }
    if (!reconstruct || slot.output) return true;
    return create_surface(value, slot.surfaces[0], frame.render_width, frame.render_height, DXGI_FORMAT_R16G16B16A16_FLOAT) &&
        create_surface(value, slot.surfaces[3], 1, 1, DXGI_FORMAT_R32_FLOAT) &&
        create_surface(value, slot.surfaces[4], frame.output_width, frame.output_height, DXGI_FORMAT_R16G16B16A16_FLOAT) &&
        rsf::create_uav_texture(device, frame.output_width, frame.output_height, DXGI_FORMAT_R16G16B16A16_FLOAT, &slot.output);
}
void report_frame_timing(const rsf_native_cpu_frame& cpu, const CpuOwner& owner, const Transfer::Slot& slot) {
    if (!cpu.qpc_frequency || !owner.ready_qpc || !owner.submit_begin || owner.submit_end < owner.submit_begin) return;
    struct Timing {
        uint64_t start = 0, frames = 0;
        std::array<double, 8> sum{}, peak{};
    };
    static thread_local Timing timing;
    const auto now = now_qpc();
    if (!timing.start) timing.start = now;
    const auto elapsed_ticks = [](uint64_t end, uint64_t begin) { return end >= begin ? end - begin : 0; };
    const uint64_t spans[]{owner.join_ticks, owner.reflex_ticks,
        elapsed_ticks(cpu.timestamps_qpc[RSF_GAME_CPU_INPUT_SAMPLE], owner.ready_qpc),
        elapsed_ticks(cpu.timestamps_qpc[RSF_GAME_CPU_SIMULATION_BEGIN], cpu.timestamps_qpc[RSF_GAME_CPU_INPUT_SAMPLE]),
        elapsed_ticks(cpu.timestamps_qpc[RSF_GAME_CPU_SIMULATION_END], cpu.timestamps_qpc[RSF_GAME_CPU_SIMULATION_BEGIN]),
        owner.submit_end - owner.submit_begin, slot.evaluate_ticks, slot.wait_ticks};
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
            "FG source timing source=%llu frames=%llu screen=%u windows_input_idle_ms=%u join_ms=%.2f/%.2f reflex_begin_ms=%.2f/%.2f engine_preinput_ms=%.2f/%.2f input_poll_ms=%.2f/%.2f simulation_ms=%.2f/%.2f rhi_ms=%.2f/%.2f sr_ms=%.2f/%.2f sr_wait_ms=%.2f/%.2f",
            static_cast<unsigned long long>(owner.id), static_cast<unsigned long long>(timing.frames), slot.pass.screen, idle,
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
extern "C" RSF_RUNTIME_API void rsf_native_fg_options_set(const rsf_native_fg_options* options) {
    if (!options || options->struct_size < sizeof(*options) || options->mode > RSF_FG_DYNAMIC ||
        options->reflex_mode > RSF_REFLEX_BOOST) return;
    std::lock_guard<std::mutex> lock(settings_guard); requested = *options;
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_reflex_ordering_set(uint32_t threaded) {
    threaded_reflex.store(threaded ? 1u : 0u, std::memory_order_relaxed);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_set_log(rsf_backend_log_fn sink, void* user) {
    std::lock_guard<std::mutex> lock(settings_guard); logger = sink; log_user = user;
}
extern "C" RSF_RUNTIME_API int rsf_native_fg_status_get(rsf_native_fg_status* status) {
    if (!status || status->struct_size < sizeof(*status)) return 0;
    std::lock_guard<std::mutex> lock(settings_guard);
    *status = cached; status->struct_size = sizeof(*status); status->requested = requested;
    status->available = rsf_d3d11_present_has_owner();
    if (!status->available) {
        status->vendor.active = 0; status->vendor.effective_mode = RSF_FG_OFF;
        status->vendor.effective_generated_frames = 0; status->vendor.low_latency_available = 0;
    }
    return 1;
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_cpu(const rsf_game_cpu_event* event) {
    if (!event || event->struct_size < sizeof(*event)) return;
    auto* host = rsf_d3d11_present_host(); if (!rsf_d3d11_present_has_owner()) return;
    const auto id = event->source_frame_id;
    const bool threaded = threaded_reflex.load(std::memory_order_relaxed) != 0;
    if (event->stage == RSF_GAME_CPU_FRAME_BEGIN) {
        const auto joined_at = now_qpc();
        const auto result = frame_acquire(id);
        const auto ready_at = now_qpc();
        trace("reserve frame before BeginFrame dispatch", id, result);
        const auto previous = last_begun.exchange(id, std::memory_order_relaxed);
        std::lock_guard<std::mutex> lock(cpu_guard);
        auto& frame = cpu_frames[id % cpu_frames.size()];
        // Another id here is at least a full ring old; a frame that never sent FRAME_END is
        // reclaimed too, or this index would refuse every later frame.
        if (frame.id != id || frame.ended) {
            frame = {}; frame.id = id; frame.failed = result != RSF_BACKEND_OK;
            frame.join_ticks = joined_at - event->timestamp_qpc;
            frame.ready_qpc = ready_at; frame.previous = previous < id ? previous : 0;
        }
    } else if (event->stage == RSF_GAME_CPU_PACING) {
        uint64_t previous = 0;
        with_frame(id, [&](const CpuOwner& frame) { previous = frame.previous; });
        // Reflex computes this sleep from the previous frame. Let its Present finish first.
        const auto joined = threaded || !host ? 0 : join_previous_present(previous);
        const auto started = now_qpc();
        uint64_t generation = 0;
        const auto result = rsf_d3d11_present_begin(id, &generation);
        const auto finished = now_qpc();
        trace("sleep at engine pacing boundary", id, result);
        with_frame(id, [&](CpuOwner& frame) {
            frame.paced = result == RSF_BACKEND_OK;
            frame.provider_generation = generation;
            frame.failed |= result != RSF_BACKEND_OK;
            frame.reflex_ticks = finished - started; frame.ready_qpc = finished;
            if (!threaded) frame.join_ticks = joined;
        });
    } else if (event->stage == RSF_GAME_CPU_INPUT_SAMPLE) {
        trace("simulation begin before input", id, frame_marker(id, RSF_LATENCY_SIMULATION_START));
    } else if (event->stage == RSF_GAME_CPU_INPUT_EVENT) {
        if (host) rsf_d3d11_present_input(id, event->input_kind, event->message_id);
        else frame_marker(id, RSF_LATENCY_INPUT_SAMPLE);
    } else if (event->stage == RSF_GAME_CPU_SIMULATION_END) {
        trace("simulation end", id, frame_marker(id, RSF_LATENCY_SIMULATION_END));
    } else if (event->stage == RSF_GAME_CPU_FRAME_END) {
        rsf_native_cpu_frame cpu{}; cpu.struct_size = sizeof(cpu);
        const bool aborted = rsf_native_cpu_read(event->session_id, id, &cpu) && (cpu.stage_mask & 15u) != 15u;
        if (aborted) frame_abort(id);
        with_frame(id, [&](CpuOwner& frame) {
            frame.ended = true; frame.expected_count = event->render_expected;
            if (aborted) frame.failed = true;
        });
        if (aborted) complete_frame(id);
        audit_frame(id);
    }
}
namespace {
// RenderSubmitEnd, once per frame. Ordered frames send it before PresentStart; the threaded
// order keeps UE's EndFrame position, after Present.
void end_render_marker(uint64_t id, const char* stage) {
    bool repeated = false;
    with_frame(id, [&](CpuOwner& frame) { repeated = frame.submit_ended; frame.submit_ended = true; });
    if (!repeated) trace(stage, id, frame_marker(id, RSF_LATENCY_RENDER_SUBMIT_END));
}
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_frame(const rsf_game_render_pass* pass, uint32_t begin) {
    if (!pass || pass->role != RSF_GAME_RENDER_FRAME || !pass->source_frame_id) return;
    auto* host = rsf_d3d11_present_host(); if (!rsf_d3d11_present_has_owner()) return;
    with_frame(pass->source_frame_id, [&](CpuOwner& frame) { if (begin) ++frame.rhi_begins; else ++frame.rhi_ends; });
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
    with_frame(id, [&](CpuOwner& frame) { if (!frame.failed && !frame.render_started) start = frame.render_started = true; });
    if (start) trace("actual graphics submit begin", id, frame_marker(id, RSF_LATENCY_RENDER_SUBMIT_START));
}
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
extern "C" RSF_RUNTIME_API void rsf_native_fg_scene(const rsf_game_render_pass* pass) { scene = pass ? *pass : rsf_game_render_pass{}; }
namespace {
// Colour formats the HUD-less copy can carry: D3D11-shareable display formats, SDR and HDR.
bool hudless_format(DXGI_FORMAT format) {
    switch (format) {
    case DXGI_FORMAT_R8G8B8A8_UNORM: case DXGI_FORMAT_R8G8B8A8_UNORM_SRGB:
    case DXGI_FORMAT_B8G8R8A8_UNORM: case DXGI_FORMAT_B8G8R8A8_UNORM_SRGB:
    case DXGI_FORMAT_R10G10B10A2_UNORM: case DXGI_FORMAT_R16G16B16A16_FLOAT:
        return true;
    default:
        return false;
    }
}
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_final(void* context, const rsf_game_render_pass* pass) {
    if (!context || !pass || !pass->scene_surface || !pass->source_frame_id || !rsf_d3d11_present_has_owner()) return;
    // HUD-less colour only feeds generation: no copy while it is off or no provider is live.
    { std::lock_guard<std::mutex> lock(settings_guard); if (requested.mode == RSF_FG_OFF) return; }
    if (!rsf_d3d11_present_provider() || !rsf_d3d11_present_session()) return;
    auto& value = transfer(); if (!initialize(value, context)) return;
    auto& slot = slot_for(value, pass->source_frame_id);
    if (!slot.prepared || !holds(slot, *pass)) return;
    slot.hudless_ready = false;
    ComPtr<ID3D11Texture2D> source;
    if (FAILED(static_cast<IUnknown*>(pass->scene_surface)->QueryInterface(IID_PPV_ARGS(&source)))) return;
    D3D11_TEXTURE2D_DESC desc{}; source->GetDesc(&desc);
    if (desc.Width != slot.output_width || desc.Height != slot.output_height || desc.SampleDesc.Count != 1 ||
        !hudless_format(desc.Format)) {
        // Generation then waits with reason HUDLESS_MISSING; say once per surface shape why.
        static uint32_t refused[4]{};
        const uint32_t shape[4]{desc.Width, desc.Height, uint32_t(desc.Format), desc.SampleDesc.Count};
        if (std::memcmp(shape, refused, sizeof(shape))) {
            std::memcpy(refused, shape, sizeof(shape));
            char text[200]; std::snprintf(text, sizeof(text), "FG HUD-less surface refused: %ux%u format %u samples %u, output %ux%u",
                desc.Width, desc.Height, uint32_t(desc.Format), desc.SampleDesc.Count, slot.output_width, slot.output_height);
            current_sink(false)(text);
        }
        return;
    }
    if (!slot.hudless || slot.hudless_format != desc.Format || slot.hudless_width != desc.Width || slot.hudless_height != desc.Height) {
        slot.hudless_width = slot.hudless_height = 0;
        if (!create_surface(value, slot.hudless, desc.Width, desc.Height, desc.Format)) return;
        slot.hudless_format = desc.Format; slot.hudless_width = desc.Width; slot.hudless_height = desc.Height;
    }
    value.context11->CopyResource(surface11(slot.hudless), source.Get());
    slot.hudless_ready = signal11(value, slot.complete);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_capture(void* context, const rsf_dlss_frame* frame) {
    if (!rsf_d3d11_present_has_owner() || !context || !frame || !scene.source_frame_id || !frame->camera_motion_included) return;
    auto& value = transfer(); if (!initialize(value, context)) return;
    auto& slot = slot_for(value, scene.source_frame_id);
    // Idempotent per frame: SR evaluation, or an earlier capture, already filled this slot for
    // this pass. Running again would also wait on this frame's own unsubmitted fence value.
    if (slot.prepared && holds(slot, scene)) return;
    if (!frame->depth || !frame->motion || !resources(value, slot, *frame, false)) return;
    slot.prepared = false;
    value.context11->CopyResource(surface11(slot.surfaces[1]), static_cast<ID3D11Resource*>(frame->depth));
    value.context11->CopyResource(surface11(slot.surfaces[2]), static_cast<ID3D11Resource*>(frame->motion));
    if (!signal11(value, slot.complete)) return;
    slot.source = scene.source_frame_id; slot.pass = scene; slot.prepared = true; slot.hudless_ready = false;
    with_frame(scene.source_frame_id, [](CpuOwner& owner) { ++owner.sr_calls; });
}
extern "C" RSF_RUNTIME_API rsf_dlss_result rsf_native_fg_evaluate(void* context, const rsf_dlss_frame* frame) {
    const auto started = now_qpc();
    if (!rsf_d3d11_present_host()) return rsf_dlss_evaluate(context, frame);
    if (!context || !frame || !scene.source_frame_id || !frame->camera_motion_included ||
        !rsf_streamline_host_token(rsf_d3d11_present_host(), scene.source_frame_id)) return RSF_DLSS_ERROR_NOT_READY;
    with_frame(scene.source_frame_id, [](CpuOwner& owner) { ++owner.sr_calls; });
    auto& value = transfer(); if (!initialize(value, context)) return RSF_DLSS_ERROR_NOT_READY;
    auto& slot = slot_for(value, scene.source_frame_id);
    if (!resources(value, slot, *frame, true)) return RSF_DLSS_ERROR_FEATURE_FAILED;
    if (FAILED(slot.allocator->Reset()) || FAILED(slot.list->Reset(slot.allocator.Get(), nullptr))) return RSF_DLSS_ERROR_FEATURE_FAILED;
    auto translated = *frame;
    // Dense motion has no unwritten sentinel. Valid zero displacement must remain valid.
    translated.motion_invalid_value = std::numeric_limits<float>::max();
    {
        // DLSS-G shares these constants. History restarts when generation starts or stops, not
        // on every frame it stays off (no provider, an ineligible frame, VSync unsupported).
        std::lock_guard<std::mutex> lock(settings_guard);
        const bool generating = cached.vendor.effective_mode != RSF_FG_OFF;
        if (requested.mode != RSF_FG_OFF && generates_on(scene.screen) && generating != was_generating) translated.reset = 1;
        was_generating = generating;
    }
    void* inputs[]{frame->color_in, frame->depth, frame->motion, frame->exposure};
    void** targets[]{&translated.color_in, &translated.depth, &translated.motion, &translated.exposure};
    for (uint32_t i = 0; i < 4; ++i) {
        if (!inputs[i]) { *targets[i] = nullptr; continue; }
        value.context11->CopyResource(surface11(slot.surfaces[i]), static_cast<ID3D11Resource*>(inputs[i]));
        *targets[i] = rsf_shared_surface_d3d12(slot.surfaces[i]);
        rsf::transition(slot.list.Get(), static_cast<ID3D12Resource*>(*targets[i]), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    }
    void* hint_inputs[]{frame->color_before_transparency, frame->transparency_layer, frame->reactive_mask,
        frame->transparency_hint, frame->bias_current_color};
    void** hint_targets[]{&translated.color_before_transparency, &translated.transparency_layer, &translated.reactive_mask,
        &translated.transparency_hint, &translated.bias_current_color};
    for (uint32_t i = 0; i < 5; ++i) {
        *hint_targets[i] = nullptr;
        if (!hint_inputs[i]) continue;
        D3D11_TEXTURE2D_DESC desc{}; static_cast<ID3D11Texture2D*>(hint_inputs[i])->GetDesc(&desc);
        auto& known = slot.hint_desc[i];
        if (!slot.hints[i] || known.Width != desc.Width || known.Height != desc.Height || known.Format != desc.Format) {
            known = {};
            // A hint that cannot cross is left untagged rather than failing the reconstruction.
            if (!create_surface(value, slot.hints[i], desc.Width, desc.Height, desc.Format)) continue;
            known = desc;
        }
        value.context11->CopyResource(surface11(slot.hints[i]), static_cast<ID3D11Resource*>(hint_inputs[i]));
        *hint_targets[i] = rsf_shared_surface_d3d12(slot.hints[i]);
        rsf::transition(slot.list.Get(), static_cast<ID3D12Resource*>(*hint_targets[i]), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    }
    translated.motion_depth_layer = nullptr; // Consumed by the D3D11 resolve already.
    translated.color_out = slot.output.Get();
    auto* queue = static_cast<ID3D12CommandQueue*>(value.graphics.queue);
    auto* fence = fence12(value);
    // This Flush stays: the D3D12 queue waits on the signal below within this frame.
    uint64_t uploaded = 0;
    if (!signal11(value, uploaded)) return RSF_DLSS_ERROR_FEATURE_FAILED;
    value.context11->Flush();
    if (FAILED(queue->Wait(fence, uploaded))) return RSF_DLSS_ERROR_FEATURE_FAILED;
    const auto result = rsf_dlss_evaluate_shared(slot.list.Get(), &translated, scene.source_frame_id);
    if (result == RSF_DLSS_OK)
        rsf::copy_transitioned(slot.list.Get(), surface12(slot.surfaces[4]), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COMMON,
            slot.output.Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    // SR only borrows colour/exposure. FG retains immutable depth and motion in the slot.
    for (uint32_t i = 0; i < 4; ++i) if (inputs[i])
        rsf::transition(slot.list.Get(), static_cast<ID3D12Resource*>(*targets[i]), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
    for (uint32_t i = 0; i < 5; ++i) if (*hint_targets[i])
        rsf::transition(slot.list.Get(), static_cast<ID3D12Resource*>(*hint_targets[i]), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
    if (FAILED(slot.list->Close())) return RSF_DLSS_ERROR_FEATURE_FAILED;
    ID3D12CommandList* lists[]{slot.list.Get()}; queue->ExecuteCommandLists(1, lists);
    slot.complete = ++value.serial;
    if (FAILED(queue->Signal(fence, slot.complete)) || FAILED(value.context11->Wait(fence11(value), slot.complete))) return RSF_DLSS_ERROR_FEATURE_FAILED;
    if (result == RSF_DLSS_OK)
        value.context11->CopyResource(static_cast<ID3D11Resource*>(frame->color_out), surface11(slot.surfaces[4]));
    // The D3D11 output consumer also belongs to this lease. A D3D12 evaluation fence alone
    // cannot authorize destroying/reallocating shared storage while this copy is outstanding.
    if (!signal11(value, slot.complete)) return RSF_DLSS_ERROR_FEATURE_FAILED;
    slot.source = scene.source_frame_id; slot.pass = scene; slot.prepared = result == RSF_DLSS_OK; slot.hudless_ready = false;
    slot.evaluate_ticks = now_qpc() - started;
    return result;
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_prepare(void*, void* chain, void*, void* list, void* buffer,
    rsf_streamline_host*, void* provider, uint32_t sync) {
    tagged_slot = nullptr;
    rsf_game_render_pass window{}; window.struct_size = sizeof(window);
    rsf_native_scene_identity final{}; final.struct_size = sizeof(final);
    auto& value = transfer();
    rsf_native_fg_options options{};
    { std::lock_guard<std::mutex> lock(settings_guard); options = requested; }
    rsf_fg_status status{}; status.struct_size = sizeof(status);
    if (!rsf_d3d11_present_provider() || !provider) {
        std::lock_guard<std::mutex> lock(settings_guard); cached.vendor = {}; cached.last_result = 0; return;
    }
    rsf_d3d11_present_provider()->status(provider, &status);
    const bool window_matched = rsf_native_window_match(chain, &window) != 0;
    const bool scene_matched = window_matched && rsf_native_scene_for_window(&window, &final);
    const uint32_t presentation_path = scene_matched ? rsf_native_scene_present_path(&window) : RSF_NATIVE_SCENE_PRESENT_NONE;
    const bool matched = presentation_path != RSF_NATIVE_SCENE_PRESENT_NONE;
    Transfer::Slot* selected = window_matched ? &slot_for(value, window.source_frame_id) : nullptr;
    rsf_native_cpu_frame cpu{}; cpu.struct_size = sizeof(cpu);
    CpuOwner cpu_owner{};
    bool submitted_frame = false;
    if (window_matched) {
        std::lock_guard<std::mutex> lock(cpu_guard);
        const auto& frame = cpu_frames[window.source_frame_id % cpu_frames.size()];
        cpu_owner = frame;
        submitted_frame = frame.id == window.source_frame_id && frame.submitted && frame.render_complete && frame.paced && !frame.failed &&
            frame.provider_generation == rsf_d3d11_present_generation();
    }
    const bool inputs = selected && selected->source == window.source_frame_id && selected->prepared;
    const bool hudless = inputs && selected->hudless_ready;
    const bool identity = selected && scene_matched && selected->pass.session_id == final.session_id &&
        selected->pass.submission_id == final.submission_id && selected->pass.view_key == final.view_key &&
        selected->pass.native_frame == final.native_frame;
    const bool scene_supported = inputs && generates_on(selected->pass.screen);
    const bool continuous = inputs && !(selected->pass.flags & RSF_GAME_RENDER_RESET);
    const bool vsync = !sync || status.vsync_supported;
    const bool cpu_valid = window_matched && rsf_native_cpu_read(window.session_id, window.source_frame_id, &cpu) &&
        !cpu.failed && (cpu.stage_mask & 7u) == 7u;
    // Slate/RHI may present while the game thread is finishing its frame. The renderer's
    // copied post-update identity authorizes these inputs; full SimulationEnd can arrive later.
    const bool valid = matched && inputs && hudless && submitted_frame && identity && scene_supported && continuous && vsync && cpu_valid;
    if (options.debug && inputs && cpu_valid && cpu_owner.id == window.source_frame_id)
        report_frame_timing(cpu, cpu_owner, *selected);
    // The requested limit is the interval of rendered frames, before generation. With
    // generation active the driver's Reflex limiter counts every presented frame (measured:
    // 10000 us requested at 2x paced rendered frames 19.9 ms apart), so it is given the interval
    // between presents. This follows the frame's eligibility, not the SDK's activity report:
    // waiting for that report left the undivided interval in place for the first generated
    // frame, which the driver doubled into one 25 ms frame at every activation. The cost of
    // deciding early is a looser limit on eligible frames the SDK is not yet generating for.
    const uint32_t generated = std::min(options.generated_frames, status.max_generated_frames);
    const uint32_t presents_per_frame = status.pacing_owner == RSF_PACING_REFLEX && valid && options.mode == RSF_FG_FIXED ? generated + 1 : 1;
    rsf_fg_options configured{sizeof(configured), RSF_FG_ABI_VERSION, valid ? options.mode : RSF_FG_OFF,
        generated, 0, status.pacing_owner == RSF_PACING_REFLEX ? options.reflex_mode : RSF_REFLEX_OFF,
        status.pacing_owner == RSF_PACING_NONE ? 0u : options.frame_limit_us / presents_per_frame};
    // Options are owner-thread calls. The provider handles transition-only SDK option updates.
    const auto configuration = rsf_d3d11_present_provider()->configure(provider, &configured);
    rsf_backend_log_fn diagnostic = nullptr; void* diagnostic_user = nullptr;
    const uint32_t eligibility = uint32_t(window_matched) | (uint32_t(scene_matched) << 1) |
        (uint32_t(matched) << 2) | (uint32_t(inputs) << 3) | (uint32_t(submitted_frame) << 4) |
        (uint32_t(identity) << 5) | (uint32_t(scene_supported) << 6) | (uint32_t(continuous) << 7) |
        (uint32_t(vsync) << 8) | (uint32_t(cpu_valid) << 9) | (presentation_path << 10) |
        ((selected ? selected->pass.screen : 0u) << 12) | (uint32_t(hudless) << 16);
    {
        std::lock_guard<std::mutex> lock(settings_guard);
        cached.vendor = status; cached.last_result = configuration; cached.source_frame_id = window.source_frame_id;
        cached.reason = !matched ? RSF_NATIVE_FG_REASON_NO_MATCH : !inputs ? RSF_NATIVE_FG_REASON_NOT_PREPARED :
            !submitted_frame ? RSF_NATIVE_FG_REASON_NO_SUBMISSION : !scene_supported ? RSF_NATIVE_FG_REASON_SCENE_UNSUPPORTED :
            !continuous ? RSF_NATIVE_FG_REASON_RESET : !vsync ? RSF_NATIVE_FG_REASON_VSYNC :
            !hudless ? RSF_NATIVE_FG_REASON_HUDLESS_MISSING : !valid ? RSF_NATIVE_FG_REASON_INVALID_INPUT : RSF_NATIVE_FG_REASON_NONE;
        if (requested.debug && eligibility != last_eligibility && eligibility_lines++ < 256) {
            last_eligibility = eligibility; diagnostic = logger; diagnostic_user = log_user;
        }
    }
    if (diagnostic) {
        char text[512];
        std::snprintf(text, sizeof(text),
            "FG eligibility source=%llu window=%u scene=%u sampled=%u direct=%u inputs=%u hudless=%u submission=%u identity=%u allowed_scene=%u continuous=%u vsync=%u cpu=%u mask=%u screen=%u flags=%u configure=%d",
            static_cast<unsigned long long>(window.source_frame_id), uint32_t(window_matched), uint32_t(scene_matched),
            uint32_t(presentation_path == RSF_NATIVE_SCENE_PRESENT_SAMPLED), uint32_t(presentation_path == RSF_NATIVE_SCENE_PRESENT_DIRECT),
            uint32_t(inputs), uint32_t(hudless), uint32_t(submitted_frame), uint32_t(identity), uint32_t(scene_supported),
            uint32_t(continuous), uint32_t(vsync), uint32_t(cpu_valid), cpu.stage_mask,
            selected ? selected->pass.screen : 0u, selected ? selected->pass.flags : 0u, configuration);
        diagnostic(diagnostic_user, text);
    }
    if (configuration != RSF_BACKEND_OK || !valid) return;
    rsf_frame_record record{}; record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = window.session_id; record.frame_id = window.source_frame_id; record.resource_generation = selected->generation;
    record.input_qpc = cpu.timestamps_qpc[RSF_GAME_CPU_INPUT_SAMPLE]; record.phase = RSF_PHASE_UI_COMPLETE; record.screen = selected->pass.screen;
    record.render_width = selected->width; record.render_height = selected->height;
    record.output_width = selected->output_width; record.output_height = selected->output_height;
    record.camera = selected->pass.camera;
    // One meaning for every provider: the source-frame delta. CPU simulation time is not a frame
    // time, and may not even have ended yet on UE's overlapped render thread.
    const float engine_delta = record.camera.frame_time_seconds * 1000;
    const auto input = record.input_qpc;
    record.frame_time_ms = std::clamp(std::isfinite(engine_delta) && engine_delta > 0 ? engine_delta :
        value.previous_input_qpc && input > value.previous_input_qpc ?
            float(double(input - value.previous_input_qpc) * 1000.0 / cpu.qpc_frequency) : 16.67f, 0.01f, 1000.0f);
    value.previous_input_qpc = input;
    record.camera.struct_size = sizeof(record.camera); record.camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    rsf_fg_frame frame{}; frame.struct_size = sizeof(frame); frame.record = &record; frame.interpolate = options.mode != RSF_FG_OFF;
    frame.motion_scale_x = frame.motion_scale_y = 1;
    // Every provider gets its inputs in the read state. Retirement restores them for D3D11 reuse
    // once the vendor's reads, which can run until interpolation at Present, have finished.
    auto* commands = static_cast<ID3D12GraphicsCommandList*>(list);
    rsf_backend_resource* resources[]{&frame.backbuffer, &frame.depth, &frame.motion, &frame.hudless};
    void* native[]{buffer, rsf_shared_surface_d3d12(selected->surfaces[1]), rsf_shared_surface_d3d12(selected->surfaces[2]),
        rsf_shared_surface_d3d12(selected->hudless)};
    for (uint32_t i = 0; i < 4; ++i) {
        const bool display = i == 0 || i == 3;
        auto& target = *resources[i]; target.struct_size = sizeof(target); target.resource = native[i]; target.generation = selected->generation;
        target.width = display ? selected->output_width : selected->width; target.height = display ? selected->output_height : selected->height;
        target.state = i ? D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE : D3D12_RESOURCE_STATE_PRESENT;
        if (i) rsf::transition(commands, static_cast<ID3D12Resource*>(native[i]), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    }
    selected->restore = true;
    const auto preparation = rsf_d3d11_present_provider()->prepare(provider, list, &frame);
    selected->prepared = preparation == RSF_BACKEND_OK;
    { std::lock_guard<std::mutex> lock(settings_guard); cached.last_result = preparation; if (selected->prepared && frame.interpolate) ++cached.tagged_frames; }
    trace("prepare/tags", window.source_frame_id, preparation);
    tagged_slot = selected;
    if (!selected->prepared) { configured.mode = RSF_FG_OFF; rsf_d3d11_present_provider()->configure(provider, &configured); }
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_retire(void*, void* provider) {
    if (!rsf_d3d11_present_provider() || !provider) {
        // No provider is left to read the inputs, but they are still in the read state.
        if (auto* slot = tagged_slot; slot && slot->restore) {
            std::lock_guard<std::mutex> lock(transfer().vendor_guard); slot->read_state = true; slot->restore = false;
        }
        tagged_slot = nullptr;
        std::lock_guard<std::mutex> lock(settings_guard); cached.vendor = {}; return;
    }
    rsf_fg_status state{}; state.struct_size = sizeof(state);
    if (rsf_d3d11_present_provider()->status(provider, &state) == RSF_BACKEND_OK) {
        rsf_backend_log_fn sink = nullptr; void* user = nullptr; bool changed = false;
        { std::lock_guard<std::mutex> lock(settings_guard);
          changed = cached.vendor.active != state.active || cached.vendor.effective_mode != state.effective_mode;
          cached.vendor = state; sink = logger; user = log_user; }
        if (changed && sink) { char text[200]; std::snprintf(text, sizeof(text), "FG effective mode=%u generated=%u active=%u total presents=%llu status=%d",
            state.effective_mode, state.effective_generated_frames, state.active, static_cast<unsigned long long>(state.total_presented), state.vendor_status); sink(user, text); }
    }
    auto* slot = tagged_slot; tagged_slot = nullptr;
    auto& value = transfer();
    bool held = false;
    { std::lock_guard<std::mutex> lock(value.vendor_guard); for (const auto& other : value.slots) held |= other.vendor_unknown; }
    // A frame without tags (FG off, an ineligible screen) still joins while slots are held, so
    // SR evaluation never waits on a retirement that only a generated frame would bring.
    if (!slot && !held) return;
    // The read-state restore runs on the provider queue, after the vendor's reads.
    const bool restoring = slot && slot->restore;
    ID3D12GraphicsCommandList* restore = nullptr;
    if (restoring && SUCCEEDED(slot->retirement_allocator->Reset()) &&
        SUCCEEDED(slot->retirement_list->Reset(slot->retirement_allocator.Get(), nullptr))) {
        for (auto* surface : {slot->surfaces[1], slot->surfaces[2], slot->hudless}) if (surface)
            rsf::transition(slot->retirement_list.Get(), surface12(surface), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
        if (SUCCEEDED(slot->retirement_list->Close())) restore = slot->retirement_list.Get();
    }
    void* fence = nullptr; uint64_t serial = 0;
    const int32_t result = restoring && !restore ? int32_t(RSF_BACKEND_ERROR_FEATURE_FAILED) :
        rsf_d3d11_present_retire(provider, restore, &fence, &serial);
    if (slot) slot->restore = false;
    {
        std::lock_guard<std::mutex> lock(value.vendor_guard);
        if (result == RSF_BACKEND_OK && fence) {
            ComPtr<ID3D12Fence> joined = static_cast<ID3D12Fence*>(fence);
            if (slot) { slot->vendor_fence = joined; slot->vendor_value = serial; slot->vendor_unknown = false; slot->read_state = false; }
            // This join also covers provider-queue reads of earlier frames, so it releases slots
            // whose own retirement failed. Their read_state stays for resources() to restore.
            for (auto& other : value.slots) if (other.vendor_unknown) {
                other.vendor_fence = joined; other.vendor_value = serial; other.vendor_unknown = false;
            }
            return;
        }
        if (!slot) return;
        // Nothing was signalled. Hold the slot rather than wait on a value that never arrives.
        // A failure before the bridge ran the list leaves the surfaces in the read state.
        slot->vendor_fence.Reset(); slot->vendor_value = 0;
        slot->vendor_unknown = true; slot->vendor_generation = rsf_d3d11_present_generation();
        slot->read_state = restoring;
    }
    trace("retirement failed; slot held until a later retirement", slot->source, result);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_present(const rsf_observer_present_event* event) {
    if (!event || event->flags & DXGI_PRESENT_TEST || !rsf_d3d11_present_has_owner()) return;
    if (!event->completed) {
        rsf_game_render_pass window{}; window.struct_size = sizeof(window);
        presenting_id = rsf_native_window_match(event->swapchain, &window) ? window.source_frame_id : 0;
        const auto counted = presenting_id ? presenting_id : rhi_frame_source;
        if (counted) with_frame(counted, [](CpuOwner& frame) {
            ++frame.presents;
            if (!presenting_id || presenting_id != rhi_frame_source) ++frame.mismatched_window;
        });
        // Slate-only output still has a real rendering interval even without a scene family.
        if (presenting_id) start_render_marker(presenting_id);
        if (presenting_id && !threaded_reflex.load(std::memory_order_relaxed))
            end_render_marker(presenting_id, "render submission end before present");
        if (presenting_id) trace("present begin", presenting_id, frame_marker(presenting_id, RSF_LATENCY_PRESENT_START));
    } else {
        if (presenting_id) {
            trace("present end", presenting_id, frame_marker(presenting_id, RSF_LATENCY_PRESENT_END));
            // Releases the next frame's sleep on the game thread.
            complete_frame(presenting_id);
        }
        presenting_id = 0;
    }
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_window_end(const rsf_game_render_pass* pass) {
    if (!pass || pass->struct_size < sizeof(*pass) || !pass->source_frame_id ||
        (rsf_d3d11_present_has_owner() && !rsf_d3d11_present_is_owner(pass->swapchain))) return;
    auto* host = rsf_d3d11_present_host();
    with_frame(pass->source_frame_id, [](CpuOwner& frame) { ++frame.windows; });
    if (rsf_d3d11_present_has_owner() && frame_live(host, pass->source_frame_id) &&
        !frame_presented(host, pass->source_frame_id)) {
        frame_abort(pass->source_frame_id);
        with_frame(pass->source_frame_id, [](CpuOwner& frame) { frame.failed = true; });
        complete_frame(pass->source_frame_id);
    }
}
