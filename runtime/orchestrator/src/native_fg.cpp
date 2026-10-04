// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_scene.h>
#include <rescaleframe/native_window.h>
#include <rescaleframe/shared_surface.h>
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
bool supported_scene(uint32_t screen) {
    return screen == RSF_SCREEN_FLIGHT || screen == RSF_SCREEN_HANGAR || screen == RSF_SCREEN_BRIEFING;
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
rsf_backend_result frame_acquire(rsf_streamline_host* host, uint64_t id) {
    (void)host; return id ? rsf_d3d11_present_acquire(id) : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
}
rsf_backend_result frame_marker(rsf_streamline_host* host, uint64_t id, rsf_latency_marker marker, uint32_t controller) {
    (void)host; return rsf_d3d11_present_marker(rsf_d3d11_present_generation(), id, marker, controller);
}
rsf_backend_result frame_abort(rsf_streamline_host* host, uint64_t id) {
    (void)host; return rsf_d3d11_present_abort(id);
}
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
        rsf_backend_log_fn sink = nullptr; void* user = nullptr;
        { std::lock_guard<std::mutex> lock(settings_guard); sink = logger; user = log_user; }
        if (sink) sink(user, text);
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
    if (!supported_scene(screen) || GetTickCount64() - since < 3000) return;
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
    bool submitted = false, ended = false, failed = false, render_complete = false, render_expected = false;
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
    rsf_backend_log_fn sink = nullptr; void* user = nullptr;
    { std::lock_guard<std::mutex> lock(settings_guard); if (requested.debug) { sink = logger; user = log_user; } }
    if (sink && text[0]) sink(user, text);
}
struct Transfer {
    ComPtr<ID3D11Device> device11;
    ComPtr<ID3D11DeviceContext4> context11;
    rsf_streamline_graphics graphics{};
    rsf_shared_fence* fence = nullptr;
    HANDLE event = nullptr;
    uint64_t serial = 0;
    uint64_t previous_input_qpc = 0;
    struct Slot {
        uint64_t source = 0, complete = 0, vendor_value = 0;
        ComPtr<ID3D12Fence> vendor_fence;
        ComPtr<ID3D12CommandAllocator> allocator;
        ComPtr<ID3D12GraphicsCommandList> list;
        ComPtr<ID3D12Resource> output;
        rsf_shared_surface* surfaces[5]{};
        rsf_shared_surface* hudless = nullptr;
        DXGI_FORMAT hudless_format = DXGI_FORMAT_UNKNOWN;
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
bool wait(Transfer& value, ID3D12Fence* fence, uint64_t serial) {
    if (!serial) return true;
    if (!fence) return false;
    const auto done = fence->GetCompletedValue();
    if (done == UINT64_MAX) return false;
    if (done >= serial) return true;
    return SUCCEEDED(fence->SetEventOnCompletion(serial, value.event)) &&
        WaitForSingleObject(value.event, 10000) == WAIT_OBJECT_0;
}
void barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* resource,
    D3D12_RESOURCE_STATES from, D3D12_RESOURCE_STATES to) {
    D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition = {resource, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, from, to}; list->ResourceBarrier(1, &b);
}
bool resources(Transfer& value, Transfer::Slot& slot, const rsf_dlss_frame& frame) {
    const auto started = now_qpc();
    if (!wait(value, static_cast<ID3D12Fence*>(rsf_shared_fence_d3d12(value.fence)), slot.complete) ||
        !wait(value, slot.vendor_fence.Get(), slot.vendor_value)) return false;
    slot.wait_ticks = now_qpc() - started;
    auto* device = static_cast<ID3D12Device*>(value.graphics.device);
    if (!slot.allocator) {
        if (FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&slot.allocator))) ||
            FAILED(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, slot.allocator.Get(), nullptr, IID_PPV_ARGS(&slot.list))) ||
            FAILED(slot.list->Close())) return false;
    }
    // Motion keeps its producer's precision: the transfer is a whole-resource copy.
    if (!frame.motion) return false;
    D3D11_TEXTURE2D_DESC motion{}; static_cast<ID3D11Texture2D*>(frame.motion)->GetDesc(&motion);
    if (motion.Format != DXGI_FORMAT_R16G16_FLOAT && motion.Format != DXGI_FORMAT_R32G32_FLOAT) return false;
    if (slot.width == frame.render_width && slot.height == frame.render_height && slot.motion_format == motion.Format &&
        slot.output_width == frame.output_width && slot.output_height == frame.output_height) return true;
    for (auto*& surface : slot.surfaces) { rsf_shared_surface_destroy(surface); surface = nullptr; }
    for (auto*& surface : slot.hints) { rsf_shared_surface_destroy(surface); surface = nullptr; }
    slot.output.Reset(); slot.width = slot.height = 0;
    for (uint32_t i = 0; i < 5; ++i) {
        const bool output = i == 4, exposure = i == 3;
        rsf_shared_surface_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_SHARED_SURFACE_ABI_VERSION;
        setup.width = output ? frame.output_width : exposure ? 1 : frame.render_width;
        setup.height = output ? frame.output_height : exposure ? 1 : frame.render_height;
        setup.format = i == 1 || exposure ? DXGI_FORMAT_R32_FLOAT : i == 2 ? motion.Format : DXGI_FORMAT_R16G16B16A16_FLOAT;
        if (rsf_shared_surface_create(value.device11.Get(), value.graphics.native_device, &setup, &slot.surfaces[i]) != RSF_SHARED_OK) return false;
    }
    D3D12_RESOURCE_DESC desc{}; desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = frame.output_width; desc.Height = frame.output_height;
    desc.DepthOrArraySize = 1; desc.MipLevels = 1; desc.SampleDesc.Count = 1;
    desc.Format = DXGI_FORMAT_R16G16B16A16_FLOAT; desc.Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    if (FAILED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc,
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS, nullptr, IID_PPV_ARGS(&slot.output)))) return false;
    slot.width = frame.render_width; slot.height = frame.render_height; slot.motion_format = motion.Format;
    slot.output_width = frame.output_width; slot.output_height = frame.output_height; ++slot.generation;
    return true;
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
    rsf_backend_log_fn sink = nullptr; void* user = nullptr;
    { std::lock_guard<std::mutex> lock(settings_guard); sink = logger; user = log_user; }
    if (sink) {
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
        sink(user, text);
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
        const auto result = frame_acquire(host, id);
        const auto ready_at = now_qpc();
        trace("reserve frame before BeginFrame dispatch", id, result);
        const auto previous = last_begun.exchange(id, std::memory_order_relaxed);
        std::lock_guard<std::mutex> lock(cpu_guard);
        auto& frame = cpu_frames[id % cpu_frames.size()];
        if (!frame.id || frame.ended) {
            frame = {}; frame.id = id; frame.failed = result != RSF_BACKEND_OK;
            frame.join_ticks = joined_at - event->timestamp_qpc;
            frame.ready_qpc = ready_at; frame.previous = previous < id ? previous : 0;
        }
    } else if (event->stage == RSF_GAME_CPU_PACING) {
        uint64_t previous = 0;
        { std::lock_guard<std::mutex> lock(cpu_guard); const auto& frame = cpu_frames[id % cpu_frames.size()];
          if (frame.id == id) previous = frame.previous; }
        // Reflex computes this sleep from the previous frame. Let its Present finish first.
        const auto joined = threaded || !host ? 0 : join_previous_present(previous);
        const auto started = now_qpc();
        uint64_t generation = 0;
        const auto result = rsf_d3d11_present_begin(id, &generation);
        const auto finished = now_qpc();
        trace("sleep at engine pacing boundary", id, result);
        std::lock_guard<std::mutex> lock(cpu_guard);
        auto& frame = cpu_frames[id % cpu_frames.size()];
        if (frame.id == id) {
            frame.paced = result == RSF_BACKEND_OK;
            frame.provider_generation = generation;
            frame.failed |= result != RSF_BACKEND_OK;
            frame.reflex_ticks = finished - started; frame.ready_qpc = finished;
            if (!threaded) frame.join_ticks = joined;
        }
    } else if (event->stage == RSF_GAME_CPU_INPUT_SAMPLE) {
        trace("simulation begin before input", id, frame_marker(host, id, RSF_LATENCY_SIMULATION_START, 0));
    } else if (event->stage == RSF_GAME_CPU_INPUT_EVENT) {
        if (host) rsf_d3d11_present_input(id, event->input_kind, event->message_id);
        else frame_marker(nullptr, id, RSF_LATENCY_INPUT_SAMPLE, 0);
    } else if (event->stage == RSF_GAME_CPU_SIMULATION_END) {
        trace("simulation end", id, frame_marker(host, id, RSF_LATENCY_SIMULATION_END, 0));
    } else if (event->stage == RSF_GAME_CPU_FRAME_END) {
        rsf_native_cpu_frame cpu{}; cpu.struct_size = sizeof(cpu);
        const bool aborted = rsf_native_cpu_read(event->session_id, id, &cpu) && (cpu.stage_mask & 15u) != 15u;
        if (aborted) frame_abort(host, id);
        {
            std::lock_guard<std::mutex> lock(cpu_guard);
            auto& frame = cpu_frames[id % cpu_frames.size()];
            if (frame.id == id) {
                frame.ended = true; frame.render_expected = event->render_expected != 0;
                frame.expected_count = event->render_expected;
                if (aborted) frame.failed = true;
            }
        }
        if (aborted) complete_frame(id);
        audit_frame(id);
    }
}
namespace {
// RenderSubmitEnd, once per frame. Ordered frames send it before PresentStart; the threaded
// order keeps UE's EndFrame position, after Present.
void end_render_marker(rsf_streamline_host* host, uint64_t id, const char* stage) {
    {
        std::lock_guard<std::mutex> lock(cpu_guard);
        auto& frame = cpu_frames[id % cpu_frames.size()];
        if (frame.id == id) { if (frame.submit_ended) return; frame.submit_ended = true; }
    }
    trace(stage, id, frame_marker(host, id, RSF_LATENCY_RENDER_SUBMIT_END, 0));
}
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_frame(const rsf_game_render_pass* pass, uint32_t begin) {
    if (!pass || pass->role != RSF_GAME_RENDER_FRAME || !pass->source_frame_id) return;
    auto* host = rsf_d3d11_present_host(); if (!rsf_d3d11_present_has_owner()) return;
    { std::lock_guard<std::mutex> lock(cpu_guard); auto& frame = cpu_frames[pass->source_frame_id % cpu_frames.size()];
      if (frame.id == pass->source_frame_id) { if (begin) ++frame.rhi_begins; else ++frame.rhi_ends; } }
    if (begin) rhi_frame_source = pass->source_frame_id;
    // BeginFrame is queued before CPU sleep/input. On the D3D12 proxy device it contains no
    // rendering yet; a driver marker here measures that empty-queue wait as GPU frame work.
    // Keep its copied identity, but open the vendor interval at actual scene submission.
    if (begin) return;
    // Presented frames closed render submission before PresentStart. This covers the rest.
    end_render_marker(host, pass->source_frame_id, "full RHI frame end");
    // A completed native frame without Present cannot be reused as a later frame's token.
    if (!begin && frame_live(host, pass->source_frame_id))
        frame_abort(host, pass->source_frame_id);
    complete_frame(pass->source_frame_id);
    if (rhi_frame_source == pass->source_frame_id) rhi_frame_source = 0;
    audit_frame(pass->source_frame_id);
}
namespace {
void start_render_marker(rsf_streamline_host* host, uint64_t id) {
    {
        std::lock_guard<std::mutex> lock(cpu_guard);
        auto& frame = cpu_frames[id % cpu_frames.size()];
        if (frame.id != id || frame.failed || frame.render_started) return;
        frame.render_started = true;
    }
    trace("actual graphics submit begin", id,
        frame_marker(host, id, RSF_LATENCY_RENDER_SUBMIT_START, 0));
}
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_submission(const rsf_game_render_pass* pass, uint32_t begin) {
    if (!pass || !pass->source_frame_id || !(pass->flags & RSF_GAME_RENDER_PRIMARY) ||
        !(pass->flags & RSF_GAME_RENDER_AFTER_SIMULATION)) return;
    auto* host = rsf_d3d11_present_host(); if (!rsf_d3d11_present_has_owner()) return;
    const auto id = pass->source_frame_id;
    watch_submission(id, pass->screen, begin != 0);
    {
        std::lock_guard<std::mutex> lock(cpu_guard); auto& frame = cpu_frames[id % cpu_frames.size()];
        if (frame.id == id) {
            if (begin) {
                if (!frame.scene_begins) { frame.first_submission = pass->submission_id; frame.first_view = pass->view_key; }
                ++frame.scene_begins; frame.last_submission = pass->submission_id; frame.last_view = pass->view_key;
                frame.screen = pass->screen; frame.native_frame = uint32_t(pass->native_frame);
            } else ++frame.scene_ends;
        }
    }
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
        for (auto old : dropped) if (old) frame_abort(host, old);
        start_render_marker(host, id);
    }
    if (!begin) {
        { std::lock_guard<std::mutex> lock(cpu_guard); auto& frame = cpu_frames[id % cpu_frames.size()];
          if (frame.id == id) { frame.submit_end = now_qpc(); frame.render_complete = true; } }
    }
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_scene(const rsf_game_render_pass* pass) { scene = pass ? *pass : rsf_game_render_pass{}; }
extern "C" RSF_RUNTIME_API void rsf_native_fg_final(void* context, const rsf_game_render_pass* pass) {
    if (!context || !pass || !pass->scene_surface || !pass->source_frame_id || !rsf_d3d11_present_has_owner()) return;
    auto& value = transfer(); if (!initialize(value, context)) return;
    auto& slot = value.slots[pass->source_frame_id % 6];
    if (!slot.prepared || slot.source != pass->source_frame_id || slot.pass.session_id != pass->session_id ||
        slot.pass.view_key != pass->view_key || slot.pass.submission_id != pass->submission_id) return;
    slot.hudless_ready = false;
    ComPtr<ID3D11Texture2D> source;
    if (FAILED(static_cast<IUnknown*>(pass->scene_surface)->QueryInterface(IID_PPV_ARGS(&source)))) return;
    D3D11_TEXTURE2D_DESC desc{}; source->GetDesc(&desc);
    if (desc.Width != slot.output_width || desc.Height != slot.output_height || desc.SampleDesc.Count != 1) return;
    if (desc.Format != DXGI_FORMAT_R8G8B8A8_UNORM && desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM &&
        desc.Format != DXGI_FORMAT_R10G10B10A2_UNORM) return;
    D3D11_TEXTURE2D_DESC target{};
    if (slot.hudless) static_cast<ID3D11Texture2D*>(rsf_shared_surface_d3d11(slot.hudless))->GetDesc(&target);
    if (!slot.hudless || slot.hudless_format != desc.Format || target.Width != desc.Width || target.Height != desc.Height) {
        rsf_shared_surface_destroy(slot.hudless); slot.hudless = nullptr;
        rsf_shared_surface_setup setup{sizeof(setup), RSF_SHARED_SURFACE_ABI_VERSION,
            desc.Width, desc.Height, uint32_t(desc.Format), 1, nullptr, nullptr};
        if (rsf_shared_surface_create(value.device11.Get(), value.graphics.native_device, &setup, &slot.hudless) != RSF_SHARED_OK) return;
        slot.hudless_format = desc.Format;
    }
    value.context11->CopyResource(static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(slot.hudless)), source.Get());
    slot.complete = ++value.serial;
    if (FAILED(value.context11->Signal(static_cast<ID3D11Fence*>(rsf_shared_fence_d3d11(value.fence)), slot.complete))) return;
    value.context11->Flush(); slot.hudless_ready = true;
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_capture(void* context, const rsf_dlss_frame* frame) {
    if (!rsf_d3d11_present_has_owner() || !context || !frame || !scene.source_frame_id || !frame->camera_motion_included) return;
    auto& value = transfer(); if (!initialize(value, context)) return;
    auto& slot = value.slots[scene.source_frame_id % 6];
    if (!resources(value, slot, *frame)) return;
    slot.prepared = false;
    void* inputs[]{frame->depth, frame->motion};
    for (uint32_t i = 0; i < 2; ++i) {
        if (!inputs[i]) return;
        value.context11->CopyResource(static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(slot.surfaces[i + 1])),
            static_cast<ID3D11Resource*>(inputs[i]));
    }
    slot.complete = ++value.serial;
    if (FAILED(value.context11->Signal(static_cast<ID3D11Fence*>(rsf_shared_fence_d3d11(value.fence)), slot.complete))) return;
    value.context11->Flush();
    slot.source = scene.source_frame_id; slot.pass = scene; slot.prepared = true; slot.hudless_ready = false;
    std::lock_guard<std::mutex> lock(cpu_guard);
    auto& owner = cpu_frames[scene.source_frame_id % cpu_frames.size()];
    if (owner.id == scene.source_frame_id) ++owner.sr_calls;
}
extern "C" RSF_RUNTIME_API rsf_dlss_result rsf_native_fg_evaluate(void* context, const rsf_dlss_frame* frame) {
    const auto started = now_qpc();
    if (!rsf_d3d11_present_host()) return rsf_dlss_evaluate(context, frame);
    if (!context || !frame || !scene.source_frame_id || !frame->camera_motion_included ||
        !rsf_streamline_host_token(rsf_d3d11_present_host(), scene.source_frame_id)) return RSF_DLSS_ERROR_NOT_READY;
    { std::lock_guard<std::mutex> lock(cpu_guard); auto& owner = cpu_frames[scene.source_frame_id % cpu_frames.size()];
      if (owner.id == scene.source_frame_id) ++owner.sr_calls; }
    auto& value = transfer(); if (!initialize(value, context)) return RSF_DLSS_ERROR_NOT_READY;
    auto& slot = value.slots[scene.source_frame_id % 6];
    if (!resources(value, slot, *frame)) return RSF_DLSS_ERROR_FEATURE_FAILED;
    if (FAILED(slot.allocator->Reset()) || FAILED(slot.list->Reset(slot.allocator.Get(), nullptr))) return RSF_DLSS_ERROR_FEATURE_FAILED;
    auto translated = *frame;
    // Dense motion has no unwritten sentinel. Valid zero displacement must remain valid.
    translated.motion_invalid_value = std::numeric_limits<float>::max();
    {
        std::lock_guard<std::mutex> lock(settings_guard);
        if (requested.mode != RSF_FG_OFF && supported_scene(scene.screen) && cached.vendor.effective_mode == RSF_FG_OFF)
            translated.reset = 1;
    }
    void* inputs[]{frame->color_in, frame->depth, frame->motion, frame->exposure};
    void** targets[]{&translated.color_in, &translated.depth, &translated.motion, &translated.exposure};
    for (uint32_t i = 0; i < 4; ++i) {
        if (!inputs[i]) { *targets[i] = nullptr; continue; }
        value.context11->CopyResource(static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(slot.surfaces[i])), static_cast<ID3D11Resource*>(inputs[i]));
        *targets[i] = rsf_shared_surface_d3d12(slot.surfaces[i]);
        barrier(slot.list.Get(), static_cast<ID3D12Resource*>(*targets[i]), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
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
            rsf_shared_surface_destroy(slot.hints[i]); slot.hints[i] = nullptr; known = {};
            rsf_shared_surface_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_SHARED_SURFACE_ABI_VERSION;
            setup.width = desc.Width; setup.height = desc.Height; setup.format = desc.Format;
            // A hint that cannot cross is left untagged rather than failing the reconstruction.
            if (rsf_shared_surface_create(value.device11.Get(), value.graphics.native_device, &setup, &slot.hints[i]) != RSF_SHARED_OK) continue;
            known = desc;
        }
        value.context11->CopyResource(static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(slot.hints[i])), static_cast<ID3D11Resource*>(hint_inputs[i]));
        *hint_targets[i] = rsf_shared_surface_d3d12(slot.hints[i]);
        barrier(slot.list.Get(), static_cast<ID3D12Resource*>(*hint_targets[i]), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    }
    translated.motion_depth_layer = nullptr; // Consumed by the D3D11 resolve already.
    translated.color_out = slot.output.Get();
    const auto uploaded = ++value.serial;
    auto* queue = static_cast<ID3D12CommandQueue*>(value.graphics.queue);
    auto* fence = static_cast<ID3D12Fence*>(rsf_shared_fence_d3d12(value.fence));
    if (FAILED(value.context11->Signal(static_cast<ID3D11Fence*>(rsf_shared_fence_d3d11(value.fence)), uploaded))) return RSF_DLSS_ERROR_FEATURE_FAILED;
    value.context11->Flush();
    if (FAILED(queue->Wait(fence, uploaded))) return RSF_DLSS_ERROR_FEATURE_FAILED;
    const auto result = rsf_dlss_evaluate_shared(slot.list.Get(), &translated, scene.source_frame_id);
    if (result == RSF_DLSS_OK) {
        auto* shared_output = static_cast<ID3D12Resource*>(rsf_shared_surface_d3d12(slot.surfaces[4]));
        barrier(slot.list.Get(), slot.output.Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
        barrier(slot.list.Get(), shared_output, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_DEST);
        slot.list->CopyResource(shared_output, slot.output.Get());
        barrier(slot.list.Get(), shared_output, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COMMON);
        barrier(slot.list.Get(), slot.output.Get(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    }
    // SR only borrows colour/exposure. FG retains immutable depth and motion in the slot.
    for (uint32_t i = 0; i < 4; ++i) if (inputs[i])
        barrier(slot.list.Get(), static_cast<ID3D12Resource*>(*targets[i]), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
    for (uint32_t i = 0; i < 5; ++i) if (*hint_targets[i])
        barrier(slot.list.Get(), static_cast<ID3D12Resource*>(*hint_targets[i]), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
    if (FAILED(slot.list->Close())) return RSF_DLSS_ERROR_FEATURE_FAILED;
    ID3D12CommandList* lists[]{slot.list.Get()}; queue->ExecuteCommandLists(1, lists);
    slot.complete = ++value.serial;
    if (FAILED(queue->Signal(fence, slot.complete)) || FAILED(value.context11->Wait(static_cast<ID3D11Fence*>(rsf_shared_fence_d3d11(value.fence)), slot.complete))) return RSF_DLSS_ERROR_FEATURE_FAILED;
    if (result == RSF_DLSS_OK)
        value.context11->CopyResource(static_cast<ID3D11Resource*>(frame->color_out), static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(slot.surfaces[4])));
    // The D3D11 output consumer also belongs to this lease. A D3D12 evaluation fence alone
    // cannot authorize destroying/reallocating shared storage while this copy is outstanding.
    slot.complete = ++value.serial;
    if (FAILED(value.context11->Signal(static_cast<ID3D11Fence*>(rsf_shared_fence_d3d11(value.fence)), slot.complete)))
        return RSF_DLSS_ERROR_FEATURE_FAILED;
    value.context11->Flush();
    slot.source = scene.source_frame_id; slot.pass = scene; slot.prepared = result == RSF_DLSS_OK; slot.hudless_ready = false;
    slot.evaluate_ticks = now_qpc() - started;
    return result;
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_prepare(void*, void* chain, void*, void* list, void* buffer,
    rsf_streamline_host* host, void* provider, uint32_t sync) {
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
    Transfer::Slot* selected = window_matched ? &value.slots[window.source_frame_id % 6] : nullptr;
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
    const bool identity = selected && scene_matched && selected->pass.session_id == final.session_id &&
        selected->pass.submission_id == final.submission_id && selected->pass.view_key == final.view_key &&
        selected->pass.native_frame == final.native_frame;
    const bool scene_supported = inputs && supported_scene(selected->pass.screen);
    const bool continuous = inputs && !(selected->pass.flags & RSF_GAME_RENDER_RESET);
    const bool vsync = !sync || status.vsync_supported;
    const bool cpu_valid = window_matched && rsf_native_cpu_read(window.session_id, window.source_frame_id, &cpu) &&
        !cpu.failed && (cpu.stage_mask & 7u) == 7u;
    // Slate/RHI may present while the game thread is finishing its frame. The renderer's
    // copied post-update identity authorizes these inputs; full SimulationEnd can arrive later.
    const bool valid = matched && inputs && submitted_frame && identity && scene_supported && continuous && vsync && cpu_valid;
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
        ((selected ? selected->pass.screen : 0u) << 12);
    {
        std::lock_guard<std::mutex> lock(settings_guard);
        cached.vendor = status; cached.last_result = configuration; cached.source_frame_id = window.source_frame_id;
        cached.reason = !matched ? 1u : !selected || !selected->prepared ? 2u : !submitted_frame ? 3u :
            !scene_supported ? 4u : selected->pass.flags & RSF_GAME_RENDER_RESET ? 5u :
            sync && !status.vsync_supported ? 6u : !valid ? 7u : 0u;
        if (requested.debug && eligibility != last_eligibility && eligibility_lines++ < 256) {
            last_eligibility = eligibility; diagnostic = logger; diagnostic_user = log_user;
        }
    }
    if (diagnostic) {
        char text[512];
        std::snprintf(text, sizeof(text),
            "FG eligibility source=%llu window=%u scene=%u sampled=%u direct=%u inputs=%u submission=%u identity=%u allowed_scene=%u continuous=%u vsync=%u cpu=%u mask=%u screen=%u flags=%u configure=%d",
            static_cast<unsigned long long>(window.source_frame_id), uint32_t(window_matched), uint32_t(scene_matched),
            uint32_t(presentation_path == RSF_NATIVE_SCENE_PRESENT_SAMPLED), uint32_t(presentation_path == RSF_NATIVE_SCENE_PRESENT_DIRECT),
            uint32_t(inputs), uint32_t(submitted_frame), uint32_t(identity), uint32_t(scene_supported),
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
    record.frame_time_ms = std::clamp(float(double(cpu.timestamps_qpc[3] - cpu.timestamps_qpc[0]) * 1000.0 / cpu.qpc_frequency), 0.01f, 1000.0f);
    record.camera = selected->pass.camera;
    if (rsf_d3d11_present_backend() != RSF_FG_BACKEND_DLSS) {
        // FSR/XeSS consume a source-frame delta, not the duration of CPU simulation (which
        // may not even have ended yet on UE's overlapped render thread).
        const float engine_delta = record.camera.frame_time_seconds * 1000;
        const auto input = record.input_qpc;
        record.frame_time_ms = std::isfinite(engine_delta) && engine_delta > 0 ? engine_delta :
            value.previous_input_qpc && input > value.previous_input_qpc ?
                float(double(input - value.previous_input_qpc) * 1000.0 / cpu.qpc_frequency) : 16.67f;
        record.frame_time_ms = std::clamp(record.frame_time_ms, 0.01f, 1000.0f);
        value.previous_input_qpc = input;
    }
    record.camera.struct_size = sizeof(record.camera); record.camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    rsf_fg_frame frame{}; frame.struct_size = sizeof(frame); frame.record = &record; frame.interpolate = options.mode != RSF_FG_OFF;
    frame.motion_scale_x = frame.motion_scale_y = 1;
    rsf_backend_resource* resources[]{&frame.backbuffer, &frame.depth, &frame.motion};
    void* native[]{buffer, rsf_shared_surface_d3d12(selected->surfaces[1]), rsf_shared_surface_d3d12(selected->surfaces[2])};
    for (uint32_t i = 0; i < 3; ++i) {
        auto& input = *resources[i]; input.struct_size = sizeof(input); input.resource = native[i]; input.generation = selected->generation;
        input.width = i ? selected->width : selected->output_width; input.height = i ? selected->height : selected->output_height;
        input.state = i ? D3D12_RESOURCE_STATE_COMMON : D3D12_RESOURCE_STATE_PRESENT;
    }
    if (selected->hudless_ready) {
        auto& input = frame.hudless; input.struct_size = sizeof(input); input.resource = rsf_shared_surface_d3d12(selected->hudless);
        input.generation = selected->generation; input.width = selected->output_width; input.height = selected->output_height;
        input.state = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        barrier(static_cast<ID3D12GraphicsCommandList*>(list), static_cast<ID3D12Resource*>(input.resource),
            D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    }
    const bool read_state = rsf_d3d11_present_backend() == RSF_FG_BACKEND_FSR3 || rsf_d3d11_present_backend() == RSF_FG_BACKEND_FSR4;
    if (read_state) for (auto* input : {&frame.depth, &frame.motion}) {
        barrier(static_cast<ID3D12GraphicsCommandList*>(list), static_cast<ID3D12Resource*>(input->resource),
            D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        input->state = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    }
    const auto preparation = rsf_d3d11_present_provider()->prepare(provider, list, &frame);
    if (frame.hudless.resource) barrier(static_cast<ID3D12GraphicsCommandList*>(list), static_cast<ID3D12Resource*>(frame.hudless.resource),
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
    if (read_state) for (auto* input : {&frame.depth, &frame.motion})
        barrier(static_cast<ID3D12GraphicsCommandList*>(list), static_cast<ID3D12Resource*>(input->resource),
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
    selected->prepared = preparation == RSF_BACKEND_OK;
    { std::lock_guard<std::mutex> lock(settings_guard); cached.last_result = preparation; if (selected->prepared && frame.interpolate) ++cached.tagged_frames; }
    trace("prepare/tags", window.source_frame_id, preparation);
    if (selected->prepared && frame.interpolate) tagged_slot = selected;
    if (!selected->prepared) { configured.mode = RSF_FG_OFF; rsf_d3d11_present_provider()->configure(provider, &configured); }
    (void)host;
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_retire(void*, void* provider) {
    if (!rsf_d3d11_present_provider() || !provider) {
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
    if (!tagged_slot) return;
    rsf_fg_retirement retirement{}; retirement.struct_size = sizeof(retirement);
    if (rsf_d3d11_present_provider()->retirement(provider, &retirement) == RSF_BACKEND_OK) {
        tagged_slot->vendor_fence = static_cast<ID3D12Fence*>(retirement.fence);
        tagged_slot->vendor_value = retirement.value;
    } else {
        // A failed retirement query forbids recycling this slot until process exit.
        tagged_slot->vendor_value = UINT64_MAX;
    }
    tagged_slot = nullptr;
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_present(const rsf_observer_present_event* event) {
    if (!event || event->flags & DXGI_PRESENT_TEST) return;
    auto* host = rsf_d3d11_present_host(); if (!rsf_d3d11_present_has_owner()) return;
    if (!event->completed) {
        rsf_game_render_pass window{}; window.struct_size = sizeof(window);
        presenting_id = rsf_native_window_match(event->swapchain, &window) ? window.source_frame_id : 0;
        { std::lock_guard<std::mutex> lock(cpu_guard);
          auto& frame = cpu_frames[(presenting_id ? presenting_id : rhi_frame_source) % cpu_frames.size()];
          if (frame.id && frame.id == (presenting_id ? presenting_id : rhi_frame_source)) {
              ++frame.presents;
              if (!presenting_id || presenting_id != rhi_frame_source) ++frame.mismatched_window;
          } }
        // Slate-only output still has a real rendering interval even without a scene family.
        if (presenting_id) start_render_marker(host, presenting_id);
        if (presenting_id && !threaded_reflex.load(std::memory_order_relaxed))
            end_render_marker(host, presenting_id, "render submission end before present");
        if (presenting_id) trace("present begin", presenting_id, frame_marker(host, presenting_id, RSF_LATENCY_PRESENT_START, 0));
    } else {
        if (presenting_id) {
            trace("present end", presenting_id, frame_marker(host, presenting_id, RSF_LATENCY_PRESENT_END, 0));
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
    { std::lock_guard<std::mutex> lock(cpu_guard); auto& frame = cpu_frames[pass->source_frame_id % cpu_frames.size()];
      if (frame.id == pass->source_frame_id) ++frame.windows; }
    if (rsf_d3d11_present_has_owner() && frame_live(host, pass->source_frame_id) &&
        !frame_presented(host, pass->source_frame_id)) {
        frame_abort(host, pass->source_frame_id);
        {
            std::lock_guard<std::mutex> lock(cpu_guard);
            auto& frame = cpu_frames[pass->source_frame_id % cpu_frames.size()];
            if (frame.id == pass->source_frame_id) frame.failed = true;
        }
        complete_frame(pass->source_frame_id);
    }
}
