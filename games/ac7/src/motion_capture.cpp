// SPDX-License-Identifier: MIT
#include <rescaleframe/ac7_hooks.h>
#include <rescaleframe/ac7_memory.h>
#include <rescaleframe/ac7_motion_capture.h>
#include "motion_capture_records.h"
#include "motion_capture_session.h"

#include <windows.h>
#include <dxgi.h>
#include <MinHook.h>

#include <algorithm>
#include <cstdio>
#include <cstring>
#include <iterator>
#include <new>
#include <utility>
#include <vector>


extern "C" bool rsf_ac7_capture_read_memory(void* destination, const void* source, size_t size)
{
    return rsf::ac7::copy_memory(destination, source, size);
}

extern "C" void rsf_ac7_capture_set_sample(bool enabled);

namespace {
using ShouldFn = bool(*)(void*, void*, bool);
using HasFn = bool(*)(void*, void*);
using HistoryFn = bool(*)(void*, void*, float*);
ShouldFn original_should = nullptr;
HasFn original_has = nullptr;
HistoryFn original_history = nullptr;
using PostProcessFn = void(*)(void*, void*, void*, void*);
using WidgetPrepareFn = void(*)(void*);
using WidgetQueueFn = void(*)(void*, float);
using TemporalFn = void(*)(void*, void*);
using GraphProcessFn = void(*)(void*, void*, void*);
PostProcessFn original_postprocess = nullptr;
WidgetPrepareFn original_widget_prepare = nullptr;
WidgetQueueFn original_widget_queue = nullptr;
TemporalFn original_add_temporal = nullptr;
TemporalFn original_temporal_process = nullptr;
GraphProcessFn original_graph_process = nullptr;

uint64_t tick()
{
    LARGE_INTEGER t{}; QueryPerformanceCounter(&t); return uint64_t(t.QuadPart);
}
void say(const char* message) { rsf_ac7_capture_log(message); }
void capture_failed(MotionCaptureInterval observed) noexcept
{
    rsf_ac7_capture_fail(observed);
}

using rsf::ac7::copy_memory;
using rsf::ac7::read;

// The typed originals stay native. Restore the Rust-owned observation scope during unwind too.
class EngineObservation {
public:
    explicit EngineObservation(EngineRecord& event) : saved(rsf_ac7_capture_swap_engine(&event)) {}
    ~EngineObservation() { rsf_ac7_capture_swap_engine(saved); }
    EngineObservation(const EngineObservation&) = delete;
    EngineObservation& operator=(const EngineObservation&) = delete;
private:
    EngineRecord* saved;
};
class RootObservation {
public:
    explicit RootObservation(const RootRecord& root) : saved(rsf_ac7_capture_enter_root(&root)) {}
    ~RootObservation() { rsf_ac7_capture_restore_root(&saved); }
    RootObservation(const RootObservation&) = delete;
    RootObservation& operator=(const RootObservation&) = delete;
private:
    NativeRootScope saved;
};

bool copy_previous(float* destination, const float* source) noexcept
{
    return copy_memory(destination, source, sizeof(EngineRecord::previous));
}
bool hooked_history(void* cache, void* primitive, float* previous)
{
    const bool result = original_history(cache, primitive, previous);
    rsf_ac7_capture_history(uint64_t(uintptr_t(primitive)), result, previous, copy_previous);
    return result;
}
bool hooked_has(void* view, void* primitive)
{
    const bool result = original_has(view, primitive);
    rsf_ac7_capture_has_velocity(uint64_t(uintptr_t(primitive)), result);
    return result;
}
bool hooked_should(void* primitive, void* view, bool check_visibility)
{
    const auto capture = rsf_ac7_capture_interval();
    if (!capture) return original_should(primitive, view, check_visibility);
    EngineRecord event{};
    event.qpc = tick(); event.primitive = uint64_t(uintptr_t(primitive));
    event.view = uint64_t(uintptr_t(view)); event.interval = capture.interval;
    event.thread = GetCurrentThreadId();
    void* proxy = nullptr; void* relevance = nullptr;
    bool valid = read(primitive, 8, proxy) && read(primitive, 0xe4, event.index) &&
                 read(primitive, 0x10, event.component) && read(view, 0x1490, relevance);
    event.proxy = uint64_t(uintptr_t(proxy));
    auto& f = event.facts;
    f.visible = 1;
    if (valid && event.index < 1048576) {
        valid = read(relevance, size_t(event.index) * 8 + 4, f.relevance) &&
                read(proxy, 0x40, f.mobility) && read(proxy, 0x118, f.radius) &&
                read(proxy, 0x100, event.origin) && read(view, 0x40c, event.camera) &&
                read(view, 0xc2c, f.lod_factor) && read(view, 0x1174, f.minimum_size) &&
                read(proxy, 0xc0, event.current);
        unsigned char cut = 0, flags = 0;
        valid = valid && read(view, 0xc34, cut) && read(proxy, 0x4a, flags);
        f.camera_cut = cut ? 1u : 0u; f.always_velocity = flags & 1u;
        if (check_visibility) {
            void* storage = nullptr;
            valid = valid && read(view, 0x1430, storage);
            if (!storage) storage = static_cast<unsigned char*>(view) + 0x1420;
            uint32_t word = 0;
            valid = valid && read(storage, size_t(event.index / 32) * 4, word);
            f.visible = (word >> (event.index % 32)) & 1u;
        }
        for (uint32_t i = 0; i < 3; ++i) {
            const float d = event.origin[i] - event.camera[i]; f.distance_squared += d * d;
        }
    } else valid = false;
    f.fields_valid = valid ? 1u : 0u;
    bool accepted;
    {
        const EngineObservation observation(event);
        accepted = original_should(primitive, view, check_visibility);
    }
    if (!rsf_ac7_capture_record_engine(capture, &event, accepted)) capture_failed(capture);
    return accepted;
}

uint32_t save_blob(MotionCaptureInterval capture, const std::vector<unsigned char>& bytes)
{
    uint32_t id;
    if (!rsf_ac7_capture_store_blob(capture, bytes.data(), bytes.size(), &id)) throw std::bad_alloc{};
    return id;
}
RootRecord root_record(MotionCaptureInterval capture, RootKind kind, void* owner)
{
    RootRecord r{};
    rsf_ac7_capture_init_root(&r, kind, uint64_t(uintptr_t(owner)));
    r.qpc = tick(); r.thread = GetCurrentThreadId();
    r.session = capture.session; r.interval = capture.interval;
    return r;
}
void record_root(const RootRecord& r) noexcept
{
    if (!rsf_ac7_capture_record_root(&r)) capture_failed({r.session, r.interval});
}
template<class Original> void observe_root(RootRecord& root, Original original)
{
    record_root(root);
    {
        const RootObservation observation(root);
        original();
    }
    rsf_ac7_capture_end_root(&root, tick());
}
void widget_fields(RootRecord& r, void* converter) noexcept
{
    // Build-specific converter layout, matched to the dump and reflected SDK. Never write it.
    r.fields_valid = read(converter, 0x28, r.draw_size) && read(converter, 0x30, r.widget) &&
        read(converter, 0x48, r.target) && read(converter, 0xc0, r.downsample) &&
        read(converter, 0xc8, r.blur_x) && read(converter, 0xd0, r.blur_y) &&
        read(converter, 0xd8, r.glow);
}
void checkpoint_native(MotionCaptureInterval capture) noexcept;
void hooked_postprocess(void* owner, void* command_list, void* view, void* velocity_reference)
{
    const auto capture = rsf_ac7_capture_interval();
    if (!capture) {
        original_postprocess(owner, command_list, view, velocity_reference); return;
    }
    auto r = root_record(capture, postprocess_begin, owner);
    r.command_list = uint64_t(uintptr_t(command_list)); r.view = uint64_t(uintptr_t(view));
    r.velocity_reference = uint64_t(uintptr_t(velocity_reference));
    r.fields_valid = read(velocity_reference, 0, r.velocity_rhi);
    // The renderer's per-view iteration establishes this AC7-specific stride. Capture its bytes
    // only at the three GPU sample intervals, without interpreting unverified view offsets.
    try {
        if (rsf_ac7_capture_sampled(r.interval)) {
            std::vector<unsigned char> bytes(0x27c0);
            if (copy_memory(bytes.data(), view, bytes.size())) r.view_blob = save_blob(capture, bytes);
        }
    } catch (...) { capture_failed(capture); }
    observe_root(r, [&] { original_postprocess(owner, command_list, view, velocity_reference); });
    record_root(r);
    checkpoint_native(capture);
}
void hooked_widget_prepare(void* converter)
{
    const auto capture = rsf_ac7_capture_interval();
    if (!capture) { original_widget_prepare(converter); return; }
    auto r = root_record(capture, widget_prepare_before, converter);
    widget_fields(r, converter);
    observe_root(r, [&] { original_widget_prepare(converter); });
    widget_fields(r, converter);
    record_root(r);
}
void hooked_widget_queue(void* converter, float delta)
{
    const auto capture = rsf_ac7_capture_interval();
    if (!capture) { original_widget_queue(converter, delta); return; }
    auto r = root_record(capture, widget_queue_before, converter);
    widget_fields(r, converter);
    observe_root(r, [&] { original_widget_queue(converter, delta); });
    widget_fields(r, converter);
    record_root(r);
}
void hooked_add_temporal(void* context, void* velocity)
{
    const auto capture = rsf_ac7_capture_interval();
    if (!capture) { original_add_temporal(context, velocity); return; }
    auto r = root_record(capture, main_temporal_build_before, context);
    r.context = uint64_t(uintptr_t(context));
    r.fields_valid = read(context, 0x10, r.view) && read(context, 0x28, r.input_node) &&
                     read(velocity, 0, r.velocity_node);
    observe_root(r, [&] { original_add_temporal(context, velocity); });
    r.fields_valid = r.fields_valid && read(context, 0x28, r.output_node);
    if (r.output_node) r.fields_valid = r.fields_valid && read(r.output_node, 0, r.vtable);
    record_root(r);
}
void hooked_temporal_process(void* node, void* context)
{
    const auto capture = rsf_ac7_capture_interval();
    if (!capture) { original_temporal_process(node, context); return; }
    auto r = root_record(capture, main_temporal_process_begin, node);
    r.context = uint64_t(uintptr_t(context)); r.output_node = r.owner;
    r.fields_valid = read(context, 0, r.view) && read(context, 0x28, r.command_list) && read(node, 0, r.vtable);
    try {
        if (rsf_ac7_capture_sampled(r.interval)) {
            std::vector<unsigned char> bytes(0xe0);
            if (copy_memory(bytes.data(), node, bytes.size())) r.node_blob = save_blob(capture, bytes);
        }
    } catch (...) { capture_failed(capture); }
    observe_root(r, [&] { original_temporal_process(node, context); });
    record_root(r);
}
void graph_output(RootRecord& r) noexcept
{
    // The matched graph executor uses GetOutput at +0x38, Process at +0x28 and
    // ComputeOutputDesc at +0x70. GetOutput is the engine's bounded, read-only member accessor.
    r.output_valid = 0; std::memset(r.output_name, 0, sizeof(r.output_name));
    auto* node = reinterpret_cast<void*>(uintptr_t(r.output_node));
    uint64_t getter = 0, output = 0, name = 0;
    if (!read(node, 0, r.vtable) || !read(r.vtable, 0x38, getter) ||
        !read(r.vtable, 0x28, r.process_fn) ||
        !read(r.vtable, 0x70, r.desc_fn) || r.output_id > 7) return;
    using GetOutputFn = void*(*)(void*, uint32_t);
#if defined(_MSC_VER)
    __try { output = uint64_t(uintptr_t(reinterpret_cast<GetOutputFn>(uintptr_t(getter))(node, r.output_id))); }
    __except(EXCEPTION_EXECUTE_HANDLER) { return; }
#else
    if (!getter) return;
    output = uint64_t(uintptr_t(reinterpret_cast<GetOutputFn>(uintptr_t(getter))(node, r.output_id)));
#endif
    auto* descriptor = reinterpret_cast<void*>(uintptr_t(output));
    if (!read(descriptor, 0x14, r.width) || !read(descriptor, 0x18, r.height) ||
        !read(descriptor, 0x2c, r.format) || !read(descriptor, 0x40, name) ||
        !read(descriptor, 0x50, r.pooled_target)) return;
    r.output_valid = 1;
    for (size_t i = 0; i < sizeof(r.output_name) - 1; ++i) {
        uint16_t c = 0;
        if (!name || !read(name, i * 2, c) || !c) break;
        r.output_name[i] = (c >= 32 && c < 127 && c != '"' && c != '\\') ? char(c) : '?';
    }
}
void associate_commands(RootRecord& r) noexcept
{
    try {
        uint64_t link = 0, command = 0;
        uint32_t uid = 0;
        auto* list = reinterpret_cast<void*>(uintptr_t(r.command_list));
        if (!read(list, 8, link) || !read(list, 0x18, uid) ||
            !rsf_ac7_capture_commands_changed(&r, link, uid) || !read(r.queue_begin, 0, command)) return;
        r.queue_end = link;
        while (command) {
            uint64_t next = 0, execute = 0;
            auto* p = reinterpret_cast<void*>(uintptr_t(command));
            if (!read(p, 0, next) || !read(p, 8, execute)) break;
            const auto advance = rsf_ac7_capture_associate_command(&r, command, execute);
            if (advance == command_failed) { capture_failed({r.session, r.interval}); return; }
            if (advance == command_stop) break;
            command = next;
        }
    } catch (...) { capture_failed({r.session, r.interval}); }
}
void hooked_graph_process(void* graph, void* output_reference, void* context)
{
    const auto capture = rsf_ac7_capture_sample();
    if (!capture) {
        original_graph_process(graph, output_reference, context); return;
    }
    auto r = root_record(capture, graph_pass_begin, graph); r.context = uint64_t(uintptr_t(context));
    r.fields_valid = read(output_reference, 0, r.output_node) && read(output_reference, 8, r.output_id) &&
        read(context, 0, r.view) && read(context, 0x28, r.command_list) &&
        read(r.view, 0, r.family) &&
        read(r.family, 0x68, r.native_frame) &&
        read(r.command_list, 8, r.queue_begin) &&
        read(r.command_list, 0x18, r.queue_uid);
    if (r.fields_valid && r.output_node) graph_output(r);
    observe_root(r, [&] { original_graph_process(graph, output_reference, context); });
    if (r.fields_valid && r.output_node) { graph_output(r); associate_commands(r); }
    record_root(r);
}
bool install_root_hooks(void** targets, uint32_t count)
{
    // Slot order matches ROOT_SITES in motion_capture_records/root_hooks.rs.
    void* detours[] = {reinterpret_cast<void*>(&hooked_postprocess),
                      reinterpret_cast<void*>(&hooked_widget_prepare), reinterpret_cast<void*>(&hooked_widget_queue),
                      reinterpret_cast<void*>(&hooked_add_temporal), reinterpret_cast<void*>(&hooked_temporal_process),
                      reinterpret_cast<void*>(&hooked_graph_process)};
    void** originals[] = {reinterpret_cast<void**>(&original_postprocess),
                         reinterpret_cast<void**>(&original_widget_prepare), reinterpret_cast<void**>(&original_widget_queue),
                         reinterpret_cast<void**>(&original_add_temporal), reinterpret_cast<void**>(&original_temporal_process),
                         reinterpret_cast<void**>(&original_graph_process)};
    static_assert(std::size(detours) == 6 && std::size(originals) == 6);
    return count == 6 && rsf::ac7::install_hooks(targets, detours, originals, count);
}
const MotionRootHookEffects root_hook_effects{
    copy_memory, install_root_hooks, rsf::ac7::remove_hooks,
};
bool install_engine_hooks(void** targets, uint32_t count)
{
    void* detours[] = {reinterpret_cast<void*>(&hooked_should), reinterpret_cast<void*>(&hooked_has),
                      reinterpret_cast<void*>(&hooked_history)};
    void** originals[] = {reinterpret_cast<void**>(&original_should), reinterpret_cast<void**>(&original_has),
                         reinterpret_cast<void**>(&original_history)};
    return count == std::size(detours) && rsf::ac7::install_hooks(targets, detours, originals, count);
}
const MotionRootHookEffects engine_hook_effects{
    copy_memory, install_engine_hooks, rsf::ac7::remove_hooks,
};
void install_roots(unsigned char* base)
{
    switch (rsf_ac7_capture_install_roots(base, &root_hook_effects)) {
    case MotionRootInstall::already_installed: return;
    case MotionRootInstall::rhi_refused:
        say("renderer root capture refused: RHI current-command write differs"); return;
    case MotionRootInstall::entry_refused:
        say("renderer root capture refused: expected AC7 entry bytes differ"); return;
    case MotionRootInstall::rolled_back:
        say("renderer root capture failed and was rolled back"); return;
    case MotionRootInstall::partial_teardown:
        say("renderer root capture has hooks pending removal"); return;
    case MotionRootInstall::installed:
        say("renderer root capture installed: graph/RHI associations, main temporal graph and widget producers; engine arguments unchanged"); return;
    }
}
bool write_file(const char* path, const unsigned char* data, size_t bytes) noexcept
{
    FILE* f = std::fopen(path, "wb");
    if (!f) return false;
    const bool ok = bytes == 0 || std::fwrite(data, 1, bytes, f) == bytes;
    return std::fclose(f) == 0 && ok;
}

void checkpoint_native(MotionCaptureInterval capture) noexcept
{
    rsf_ac7_capture_publish_checkpoint(write_file, capture);
}

MotionArtifactStats finish_stats() noexcept
{
    LARGE_INTEGER frequency{}; QueryPerformanceFrequency(&frequency);
    return {frequency.QuadPart, rsf_ac7_capture_engine_installed(), rsf_ac7_capture_current_command_address() != nullptr,
        rsf_ac7_capture_failures()};
}
bool poll_capture(void* swapchain, bool* pressed) noexcept
{
    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(static_cast<IDXGISwapChain*>(swapchain)->GetDesc(&desc))) return false;
    DWORD process = 0; GetWindowThreadProcessId(GetForegroundWindow(), &process);
    *pressed = process == GetCurrentProcessId() && (GetAsyncKeyState(VK_F9) & 0x8000) != 0;
    return true;
}

void capture_stamp(MotionCaptureStamp* stamp) noexcept
{
    SYSTEMTIME time{}; GetLocalTime(&time);
    *stamp = {time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond,
        uint32_t(GetCurrentProcessId())};
}

bool create_capture_directory(const char* path) noexcept
{
    return CreateDirectoryA(path, nullptr) != FALSE;
}

const MotionCaptureEffects capture_effects{
    poll_capture, capture_stamp, create_capture_directory, rsf_ac7_capture_set_sample, write_file, finish_stats,
};
} // namespace

extern "C" int rsf_ac7_motion_capture_install(void)
{
    if (rsf_ac7_capture_engine_installed()) return 1;
    if (!rsf_ac7_capture_enabled()) return 0;
    auto* base = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr));
    const MH_STATUS init = MH_Initialize();
    if (init != MH_OK && init != MH_ERROR_ALREADY_INITIALIZED) { say("motion capture: MinHook unavailable"); return 0; }
    install_roots(base);
    switch (rsf_ac7_capture_install_engine(base, &engine_hook_effects)) {
    case MotionRootInstall::already_installed: return 1;
    case MotionRootInstall::entry_refused:
    case MotionRootInstall::rhi_refused:
        say("motion capture engine hooks refused: expected AC7 bytes differ"); return 0;
    case MotionRootInstall::rolled_back:
        say("motion capture engine hooks failed and were rolled back"); return 0;
    case MotionRootInstall::partial_teardown:
        say("motion capture engine hooks pending removal"); return 0;
    case MotionRootInstall::installed:
        say("motion capture engine hooks installed; original decisions preserved"); return 1;
    }
    return 0;
}
extern "C" void rsf_ac7_motion_capture_shutdown(void)
{
    rsf_ac7_capture_shutdown(&capture_effects);
    if (!rsf_ac7_capture_remove_engine(&engine_hook_effects))
        say("motion capture engine hooks pending removal");
    if (!rsf_ac7_capture_remove_roots(&root_hook_effects))
        say("renderer root capture shutdown left hooks pending removal");
}
extern "C" void rsf_ac7_motion_capture_native_pass(const rsf_game_render_pass* pass, uint32_t begin)
{
    if (!rsf_ac7_capture_execution_pass(pass)) return;
    const auto capture = rsf_ac7_capture_sample();
    if (!capture) return;
    auto r = root_record(capture, begin ? rhi_scope_begin : rhi_scope_restore, nullptr);
    rsf_ac7_capture_execution_root(&r);
    record_root(r);
}
extern "C" int rsf_ac7_motion_capture_present(void* swapchain, char* prefix, uint32_t capacity)
{
    return rsf_ac7_capture_present(swapchain, prefix, capacity, &capture_effects);
}
