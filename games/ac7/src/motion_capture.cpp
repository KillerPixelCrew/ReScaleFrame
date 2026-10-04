// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/ac7_motion_capture.h>
#include <rescaleframe/constant_buffer_read.h>
#include <rescaleframe/frame_tap.h>
#include <rescaleframe/ui_identify.h>
#include <rescaleframe/texture_dump.h>

#include <windows.h>
#include <d3d11.h>
#include <d3dcompiler.h>
#include <dxgi.h>
#include <MinHook.h>
#include <wrl/client.h>

#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <sstream>
#include <string>
#include <unordered_map>
#include <vector>

using Microsoft::WRL::ComPtr;

extern "C" const char* rsf_ac7_velocity_reason(const rsf_ac7_velocity_facts* f)
{
    if (!f || !f->fields_valid) return "unreadable";
    if (f->accepted) return "accepted";
    if (!f->visible) return "visibility";
    if (f->mobility < 1 || f->mobility > 2) return "mobility";
    if ((f->relevance & 0x4001u) != 0x4001u) return "material_or_main_pass";
    const float threshold = f->minimum_size * 0.02f;
    if (!std::isfinite(f->radius) || !std::isfinite(f->distance_squared) ||
        !std::isfinite(f->lod_factor) || !std::isfinite(threshold)) return "nonfinite";
    if (f->radius * f->radius <= f->distance_squared * (f->lod_factor * f->lod_factor) *
        (threshold * threshold)) return "size";
    if (f->has_velocity_called && !f->has_velocity) {
        if (f->camera_cut) return "camera_cut";
        if (f->history_checked && !f->history_found) return "missing_history";
        if (f->history_found && !f->always_velocity) return "unchanged_transform";
        return "has_velocity_other";
    }
    return "routing_or_other";
}

namespace {
constexpr uint32_t max_engine = 16384;
constexpr uint32_t max_roots = 2048;
constexpr uint32_t max_commands = 16384;
constexpr uint32_t max_draws = 12000;
constexpr uint32_t max_blobs = 4096;
constexpr size_t shader_budget = 64u * 1024u * 1024u;
constexpr size_t blob_budget = 32u * 1024u * 1024u;
constexpr uint32_t cloud_composite_shader = 0x83524e47u;
// Captured directional light variant: screen-space contact ray and stationary IGN.
constexpr uint32_t contact_light_shader = 3619939816u;
constexpr uint32_t contact_light_phased_shader = 4154163049u; // AC7 transform: noise phase and depth quantisation bias

struct EngineRecord {
    rsf_ac7_velocity_facts facts{};
    uint64_t qpc = 0, primitive = 0, proxy = 0, view = 0;
    uint32_t interval = 0, thread = 0, component = 0, index = 0, history_called = 0;
    float camera[3]{}, origin[3]{}, current[16]{}, previous[16]{};
};
struct RootRecord {
    const char* kind = nullptr;
    uint64_t id = 0, parent = 0, qpc = 0, owner = 0, command_list = 0, view = 0;
    uint64_t velocity_reference = 0, velocity_rhi = 0, widget = 0, target = 0;
    uint64_t context = 0, input_node = 0, output_node = 0, velocity_node = 0, vtable = 0;
    uint64_t family = 0, pooled_target = 0, queue_begin = 0, queue_end = 0, process_fn = 0, desc_fn = 0;
    uint64_t downsample = 0, blur_x = 0, blur_y = 0, glow = 0;
    uint32_t session = 0, interval = 0, thread = 0, fields_valid = 0, view_blob = 0;
    uint32_t node_blob = 0;
    uint32_t native_frame = 0, queue_uid = 0, output_id = 0, width = 0, height = 0, format = 0;
    uint32_t commands = 0, output_valid = 0, native_role = 0;
    uint64_t execution_scope = 0, color_input = 0, color_output = 0, depth = 0, motion = 0;
    char output_name[64]{};
    int32_t draw_size[2]{};
};
struct Bytes { std::vector<unsigned char> data; uint64_t serial = 0; };
struct Shader {
    void* pointer = nullptr; uint32_t stage = 0, hash = 0, constant_mask = 0x3fffu;
    std::vector<unsigned char> data;
};
struct QueuedScope {
    uint64_t scope = 0, view = 0, family = 0, command_list = 0, execute = 0;
    uint32_t native_frame = 0, queue_uid = 0;
};
struct Capture {
    std::mutex guard;
    bool configured = false, key_down = false;
    std::atomic<bool> installed{false};
    std::atomic<bool> roots_installed{false};
    std::atomic<uint64_t> root_serial{0};
    std::atomic<bool> active{false};
    std::atomic<uint32_t> interval{0};
    std::atomic<uint32_t> session{0};
    std::atomic<bool> request{false};
    uint64_t presents = 0, start = 0, upload_serial = 0;
    std::string directory, prefix;
    rsf_ac7_capture_log_fn log = nullptr;
    void* user = nullptr;
    std::vector<EngineRecord> engine;
    std::vector<RootRecord> roots;
    std::unordered_map<uint64_t, QueuedScope> commands;
    std::vector<std::string> draws;
    std::vector<std::vector<unsigned char>> blobs;
    std::unordered_map<void*, Bytes> buffers;
    std::vector<Shader> shaders;
    size_t shader_bytes = 0, blob_bytes = 0;
    uint32_t engine_dropped = 0, draw_dropped = 0, blob_dropped = 0, shaders_dropped = 0;
    uint32_t roots_dropped = 0;
    uint32_t commands_dropped = 0;
    uint32_t readbacks = 0, readback_failed = 0;
    uint32_t readback_interval = UINT32_MAX, interval_readbacks = 0;
    uint32_t lighting_readbacks = 0;
    uint32_t lighting_interval = UINT32_MAX;
    bool lighting_after_pending = false;
    uint32_t colour_readbacks = 0, colour_readback_failed = 0;
    std::atomic<uint32_t> failures{0};
};
Capture& state() { static Capture s; return s; }
std::atomic<bool> native_owner{false};
thread_local rsf_game_render_pass execution_pass{};
using ShouldFn = bool(*)(void*, void*, bool);
using HasFn = bool(*)(void*, void*);
using HistoryFn = bool(*)(void*, void*, float*);
ShouldFn original_should = nullptr;
HasFn original_has = nullptr;
HistoryFn original_history = nullptr;
void* hook_targets[3]{};
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
void* root_targets[6]{};
void* current_rhi_command_address = nullptr;
thread_local uint64_t current_root = 0;
thread_local void* current_postprocess_view = nullptr;
thread_local EngineRecord* current_event = nullptr;

uint64_t tick()
{
    LARGE_INTEGER t{}; QueryPerformanceCounter(&t); return uint64_t(t.QuadPart);
}
void say(const char* message) { auto& s = state(); if (s.log) s.log(s.user, message); }
void capture_failed()
{
    state().failures.fetch_add(1);
    state().active.store(false, std::memory_order_release);
    say("motion capture stopped after an allocation or serialization failure; game decisions are unchanged");
}

// No C++ objects in the SEH scope. These are live engine arguments, but a mismatched layout
// must produce an unreadable record rather than a second fault inside diagnostic code.
bool copy_memory(void* out, const void* in, size_t bytes)
{
    if (!in || !out) return false;
#if defined(_MSC_VER)
    __try { std::memcpy(out, in, bytes); return true; }
    __except(EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION m{};
    if (!VirtualQuery(in, &m, sizeof(m)) || m.State != MEM_COMMIT ||
        (m.Protect & (PAGE_NOACCESS | PAGE_GUARD)) ||
        uintptr_t(in) + bytes > uintptr_t(m.BaseAddress) + m.RegionSize) return false;
    std::memcpy(out, in, bytes); return true;
#endif
}
template<class T> bool read(void* p, size_t offset, T& out)
{ return p && copy_memory(&out, static_cast<unsigned char*>(p) + offset, sizeof(T)); }

bool hooked_history(void* cache, void* primitive, float* previous)
{
    const bool result = original_history(cache, primitive, previous);
    if (current_event && current_event->primitive == uint64_t(uintptr_t(primitive))) {
        current_event->facts.history_checked = 1;
        current_event->facts.history_found = result ? 1u : 0u;
        current_event->history_called = 1;
        if (result && !copy_memory(current_event->previous, previous, sizeof(current_event->previous)))
            current_event->facts.fields_valid = 0;
    }
    return result;
}
bool hooked_has(void* view, void* primitive)
{
    const bool result = original_has(view, primitive);
    if (current_event && current_event->primitive == uint64_t(uintptr_t(primitive))) {
        current_event->facts.has_velocity_called = 1;
        current_event->facts.has_velocity = result ? 1u : 0u;
    }
    return result;
}
bool hooked_should(void* primitive, void* view, bool check_visibility)
{
    auto& s = state();
    if (!s.active.load(std::memory_order_acquire)) return original_should(primitive, view, check_visibility);
    const uint32_t session = s.session.load();
    EngineRecord event{};
    event.qpc = tick(); event.primitive = uint64_t(uintptr_t(primitive));
    event.view = uint64_t(uintptr_t(view)); event.interval = s.interval.load();
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
    EngineRecord* saved = current_event; current_event = &event;
    f.accepted = original_should(primitive, view, check_visibility) ? 1u : 0u;
    current_event = saved;
    try {
        std::lock_guard<std::mutex> lock(s.guard);
        if (s.active.load() && session == s.session.load()) {
            if (s.engine.size() < max_engine) s.engine.push_back(event); else ++s.engine_dropped;
        }
    } catch (...) { capture_failed(); }
    return f.accepted != 0;
}

template<size_t N> void floats(std::ostream& out, const float (&values)[N])
{
    out << '[';
    for (size_t i = 0; i < N; ++i) {
        if (i) out << ',';
        if (std::isfinite(values[i])) out << values[i]; else out << "null";
    }
    out << ']';
}
uint32_t save_blob(std::vector<unsigned char> bytes)
{
    auto& s = state();
    std::lock_guard<std::mutex> lock(s.guard);
    for (size_t i = 0; i < s.blobs.size(); ++i)
        if (s.blobs[i] == bytes) return uint32_t(i + 1);
    if (s.blobs.size() >= max_blobs || s.blob_bytes + bytes.size() > blob_budget) {
        ++s.blob_dropped; return 0;
    }
    s.blob_bytes += bytes.size(); s.blobs.push_back(std::move(bytes));
    return uint32_t(s.blobs.size());
}
RootRecord root_record(const char* kind, void* owner)
{
    auto& s = state();
    RootRecord r{}; r.kind = kind; r.owner = uint64_t(uintptr_t(owner));
    r.id = s.root_serial.fetch_add(1) + 1; r.parent = current_root;
    r.qpc = tick(); r.thread = GetCurrentThreadId();
    r.session = s.session.load(); r.interval = s.interval.load();
    return r;
}
void record_root(const RootRecord& r) noexcept
{
    try {
        auto& s = state(); std::lock_guard<std::mutex> lock(s.guard);
        if (!s.active.load() || r.session != s.session.load()) return;
        if (s.roots.size() < max_roots) s.roots.push_back(r); else ++s.roots_dropped;
    } catch (...) { capture_failed(); }
}
void widget_fields(RootRecord& r, void* converter) noexcept
{
    // Build-specific converter layout, matched to the dump and reflected SDK. Never write it.
    r.fields_valid = read(converter, 0x28, r.draw_size) && read(converter, 0x30, r.widget) &&
        read(converter, 0x48, r.target) && read(converter, 0xc0, r.downsample) &&
        read(converter, 0xc8, r.blur_x) && read(converter, 0xd0, r.blur_y) &&
        read(converter, 0xd8, r.glow);
}
void checkpoint_native() noexcept;
void hooked_postprocess(void* owner, void* command_list, void* view, void* velocity_reference)
{
    auto& s = state();
    if (!s.active.load(std::memory_order_acquire)) {
        original_postprocess(owner, command_list, view, velocity_reference); return;
    }
    auto r = root_record("postprocess_begin", owner);
    r.command_list = uint64_t(uintptr_t(command_list)); r.view = uint64_t(uintptr_t(view));
    r.velocity_reference = uint64_t(uintptr_t(velocity_reference));
    r.fields_valid = read(velocity_reference, 0, r.velocity_rhi);
    // The renderer's per-view iteration establishes this AC7-specific stride. Capture its bytes
    // only at the three GPU sample intervals, without interpreting unverified view offsets.
    try {
        if (r.interval == 0 || r.interval == 30 || r.interval == 59) {
            std::vector<unsigned char> bytes(0x27c0);
            if (copy_memory(bytes.data(), view, bytes.size())) r.view_blob = save_blob(std::move(bytes));
        }
    } catch (...) { capture_failed(); }
    record_root(r);
    const auto saved_root = current_root; void* saved_view = current_postprocess_view;
    current_root = r.id; current_postprocess_view = view;
    original_postprocess(owner, command_list, view, velocity_reference);
    current_root = saved_root; current_postprocess_view = saved_view;
    r.kind = "postprocess_end"; r.qpc = tick(); record_root(r);
    if (r.interval == 0 || r.interval == 30 || r.interval == 59) checkpoint_native();
}
void hooked_widget_prepare(void* converter)
{
    if (!state().active.load(std::memory_order_acquire)) { original_widget_prepare(converter); return; }
    auto r = root_record("widget_prepare_before", converter); widget_fields(r, converter); record_root(r);
    original_widget_prepare(converter);
    r.kind = "widget_prepare_after"; r.qpc = tick(); widget_fields(r, converter); record_root(r);
}
void hooked_widget_queue(void* converter, float delta)
{
    if (!state().active.load(std::memory_order_acquire)) { original_widget_queue(converter, delta); return; }
    auto r = root_record("widget_queue_before", converter); widget_fields(r, converter); record_root(r);
    const auto saved = current_root; current_root = r.id;
    original_widget_queue(converter, delta);
    current_root = saved;
    r.kind = "widget_queue_after"; r.qpc = tick(); widget_fields(r, converter); record_root(r);
}
void hooked_add_temporal(void* context, void* velocity)
{
    if (!state().active.load(std::memory_order_acquire)) { original_add_temporal(context, velocity); return; }
    auto r = root_record("main_temporal_build_before", context);
    r.context = uint64_t(uintptr_t(context));
    r.fields_valid = read(context, 0x10, r.view) && read(context, 0x28, r.input_node) &&
                     read(velocity, 0, r.velocity_node);
    record_root(r);
    original_add_temporal(context, velocity);
    r.kind = "main_temporal_build_after"; r.qpc = tick();
    r.fields_valid = r.fields_valid && read(context, 0x28, r.output_node);
    if (r.output_node) r.fields_valid = r.fields_valid && read(reinterpret_cast<void*>(uintptr_t(r.output_node)), 0, r.vtable);
    record_root(r);
}
void hooked_temporal_process(void* node, void* context)
{
    if (!state().active.load(std::memory_order_acquire)) { original_temporal_process(node, context); return; }
    auto r = root_record("main_temporal_process_begin", node);
    r.context = uint64_t(uintptr_t(context)); r.output_node = r.owner;
    r.fields_valid = read(context, 0, r.view) && read(context, 0x28, r.command_list) && read(node, 0, r.vtable);
    try {
        if (r.interval == 0 || r.interval == 30 || r.interval == 59) {
            std::vector<unsigned char> bytes(0xe0);
            if (copy_memory(bytes.data(), node, bytes.size())) r.node_blob = save_blob(std::move(bytes));
        }
    } catch (...) { capture_failed(); }
    record_root(r);
    const auto saved_root = current_root; void* saved_view = current_postprocess_view;
    current_root = r.id; current_postprocess_view = reinterpret_cast<void*>(uintptr_t(r.view));
    original_temporal_process(node, context);
    current_root = saved_root; current_postprocess_view = saved_view;
    r.kind = "main_temporal_process_end"; r.qpc = tick(); record_root(r);
}
void graph_output(RootRecord& r) noexcept
{
    // The matched graph executor uses GetOutput at +0x38, Process at +0x28 and
    // ComputeOutputDesc at +0x70. GetOutput is the engine's bounded, read-only member accessor.
    r.output_valid = 0; std::memset(r.output_name, 0, sizeof(r.output_name));
    auto* node = reinterpret_cast<void*>(uintptr_t(r.output_node));
    uint64_t getter = 0, output = 0, name = 0;
    if (!read(node, 0, r.vtable) || !read(reinterpret_cast<void*>(uintptr_t(r.vtable)), 0x38, getter) ||
        !read(reinterpret_cast<void*>(uintptr_t(r.vtable)), 0x28, r.process_fn) ||
        !read(reinterpret_cast<void*>(uintptr_t(r.vtable)), 0x70, r.desc_fn) || r.output_id > 7) return;
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
        if (!name || !read(reinterpret_cast<void*>(uintptr_t(name)), i * 2, c) || !c) break;
        r.output_name[i] = (c >= 32 && c < 127 && c != '"' && c != '\\') ? char(c) : '?';
    }
}
void associate_commands(RootRecord& r, uint64_t first_link) noexcept
{
    try {
        auto& s = state(); uint64_t link = 0, command = 0;
        uint32_t uid = 0;
        auto* list = reinterpret_cast<void*>(uintptr_t(r.command_list));
        if (!read(list, 8, link) || !read(list, 0x18, uid) || uid != r.queue_uid ||
            !first_link || first_link == link || !read(reinterpret_cast<void*>(uintptr_t(first_link)), 0, command)) return;
        r.queue_end = link;
        for (uint32_t i = 0; command && i < 4096; ++i) {
            uint64_t next = 0, execute = 0;
            auto* p = reinterpret_cast<void*>(uintptr_t(command));
            if (!read(p, 0, next) || !read(p, 8, execute)) break;
            {
                std::lock_guard<std::mutex> lock(s.guard);
                if (!s.active.load() || r.session != s.session.load()) return;
                const auto found = s.commands.find(command);
                // Nested children finish first. Preserve their narrower owner within this same
                // command-list generation; replace stale addresses after reset/reuse.
                if (found == s.commands.end() || found->second.queue_uid != uid ||
                    found->second.command_list != r.command_list || found->second.family != r.family) {
                    if (found != s.commands.end() || s.commands.size() < max_commands) {
                        s.commands[command] = {r.id, r.view, r.family, r.command_list, execute, r.native_frame, uid};
                    } else ++s.commands_dropped;
                }
            }
            ++r.commands;
            if (command == link) break; // Next is the first field, so the final link names the tail.
            command = next;
        }
    } catch (...) { capture_failed(); }
}
void hooked_graph_process(void* graph, void* output_reference, void* context)
{
    auto& s = state(); const auto interval = s.interval.load();
    if (!s.active.load(std::memory_order_acquire) || (interval != 0 && interval != 30 && interval != 59)) {
        original_graph_process(graph, output_reference, context); return;
    }
    auto r = root_record("graph_pass_begin", graph); r.context = uint64_t(uintptr_t(context));
    uint64_t first_link = 0;
    r.fields_valid = read(output_reference, 0, r.output_node) && read(output_reference, 8, r.output_id) &&
        read(context, 0, r.view) && read(context, 0x28, r.command_list) &&
        read(reinterpret_cast<void*>(uintptr_t(r.view)), 0, r.family) &&
        read(reinterpret_cast<void*>(uintptr_t(r.family)), 0x68, r.native_frame) &&
        read(reinterpret_cast<void*>(uintptr_t(r.command_list)), 8, first_link) &&
        read(reinterpret_cast<void*>(uintptr_t(r.command_list)), 0x18, r.queue_uid);
    r.queue_begin = first_link;
    if (r.fields_valid && r.output_node) graph_output(r);
    record_root(r);
    const auto saved_root = current_root; void* saved_view = current_postprocess_view;
    current_root = r.id; current_postprocess_view = reinterpret_cast<void*>(uintptr_t(r.view));
    original_graph_process(graph, output_reference, context);
    current_root = saved_root; current_postprocess_view = saved_view;
    r.kind = "graph_pass_end"; r.qpc = tick();
    if (r.fields_valid && r.output_node) { graph_output(r); associate_commands(r, first_link); }
    record_root(r);
}
void queued_scope(std::ostream& out)
{
    uint64_t command = 0, execute = 0;
    QueuedScope scope{};
    if (state().roots_installed && copy_memory(&command, current_rhi_command_address, sizeof(command)) && command &&
        read(reinterpret_cast<void*>(uintptr_t(command)), 8, execute)) {
        auto& s = state(); std::lock_guard<std::mutex> lock(s.guard);
        const auto found = s.commands.find(command);
        if (found != s.commands.end() && found->second.execute == execute) scope = found->second;
    }
    if (execution_pass.scope_id) {
        scope.scope = execution_pass.scope_id; scope.view = execution_pass.view_key;
        scope.family = execution_pass.family_key; scope.native_frame = uint32_t(execution_pass.native_frame);
    }
    out << ",\"rhi_command\":" << command << ",\"rhi_execute\":" << execute
        << ",\"queued_native_scope\":" << scope.scope << ",\"queued_native_view\":" << scope.view
        << ",\"queued_native_family\":" << scope.family << ",\"queued_native_frame\":" << scope.native_frame
        << ",\"queued_list\":" << scope.command_list << ",\"queued_uid\":" << scope.queue_uid;
}
void install_roots(unsigned char* base)
{
    auto& s = state(); if (s.roots_installed) return;
    // Short stable entry sequences exclude security-cookie RIP displacements. MinHook relocates
    // complete instructions; these guards establish that each target is the researched entry.
    const unsigned char expected[][20] = {
        {0x4c,0x8b,0xdc,0x55,0x49,0x8d,0xab,0xe8,0xfc,0xff,0xff,0x48,0x81,0xec,0x10,0x04,0x00,0x00},
        {0x4c,0x8b,0xdc,0x55,0x48,0x81,0xec,0x10,0x03,0x00,0x00},
        {0x40,0x55,0x57,0x48,0x8d,0x6c,0x24,0xa8,0x48,0x81,0xec,0x58,0x01,0x00,0x00},
        {0x48,0x89,0x5c,0x24,0x10,0x48,0x89,0x7c,0x24,0x18,0x55,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57},
        {0x40,0x55,0x53,0x56,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57,0x48,0x8d,0xac,0x24,0xf0,0xfd,0xff,0xff},
        {0x4c,0x89,0x44,0x24,0x18,0x48,0x89,0x4c,0x24,0x08,0x53,0x48,0x83,0xec,0x30}
    };
    const size_t lengths[] = {18, 11, 15, 19, 20, 15};
    const uint32_t rvas[] = {0xffb900, 0x4d69a0, 0x4d6340, 0xff6ad0, 0x10aeca0, 0x10b3f10};
    unsigned char actual[20]{};
    if (!base || !copy_memory(actual, base + 0x120e67c, 7) ||
        std::memcmp(actual, "\x48\x89\x05\xd5\x9c\xa6\x02", 7)) {
        say("renderer root capture refused: RHI current-command write differs"); return;
    }
    const uint32_t first = native_owner.load() ? 1u : 0u;
    for (size_t i = first; i < 6; ++i) {
        if (i == 2 && native_owner.load()) continue;
        if (!base || !copy_memory(actual, base + rvas[i], lengths[i]) ||
            std::memcmp(actual, expected[i], lengths[i])) {
            say("renderer root capture refused: expected AC7 entry bytes differ"); return;
        }
    }
    void* detours[] = {reinterpret_cast<void*>(&hooked_postprocess),
                      reinterpret_cast<void*>(&hooked_widget_prepare), reinterpret_cast<void*>(&hooked_widget_queue),
                      reinterpret_cast<void*>(&hooked_add_temporal), reinterpret_cast<void*>(&hooked_temporal_process),
                      reinterpret_cast<void*>(&hooked_graph_process)};
    void** originals[] = {reinterpret_cast<void**>(&original_postprocess),
                         reinterpret_cast<void**>(&original_widget_prepare), reinterpret_cast<void**>(&original_widget_queue),
                         reinterpret_cast<void**>(&original_add_temporal), reinterpret_cast<void**>(&original_temporal_process),
                         reinterpret_cast<void**>(&original_graph_process)};
    uint32_t made = first;
    for (; made < 6; ++made) {
        if (made == 2 && native_owner.load()) continue;
        root_targets[made] = base + rvas[made];
        if (MH_CreateHook(root_targets[made], detours[made], originals[made]) != MH_OK) break;
    }
    if (made == 6) {
        bool enabled = true;
        for (auto* target : root_targets) if (target) enabled = MH_EnableHook(target) == MH_OK && enabled;
        if (enabled) {
            s.roots_installed = true;
            current_rhi_command_address = base + 0x3c78358;
            say("renderer root capture installed: graph/RHI associations, main temporal graph and widget producers; engine arguments unchanged"); return;
        }
    }
    for (uint32_t i = first; i < made; ++i) { MH_DisableHook(root_targets[i]); MH_RemoveHook(root_targets[i]); }
    say("renderer root capture failed and was rolled back");
}
void constants(std::ostream& out, ID3D11DeviceContext* context, void* const* buffers,
               uint32_t mask = 0x3fffu, bool lighting = false)
{
    auto& s = state();
    const auto interval = s.interval.load();
    if (s.readback_interval != interval) {
        s.readback_interval = interval; s.interval_readbacks = 0; s.lighting_readbacks = 0;
    }
    out << '['; bool first = true;
    for (uint32_t slot = 0; slot < 14; ++slot) {
        if (!buffers[slot] || !(mask & (1u << slot))) continue;
        std::vector<unsigned char> bytes;
        uint64_t serial = 0;
        {
            std::lock_guard<std::mutex> lock(s.guard);
            const auto found = s.buffers.find(buffers[slot]);
            if (found != s.buffers.end()) { bytes = found->second.data; serial = found->second.serial; }
        }
        if (lighting) { bytes.clear(); serial = 0; }
        bool fallback = false;
        // Reserve eight of the existing 128 reads for the verified lighting producer.
        // UI and base-pass reads otherwise exhaust the quota before its View/light values.
        const bool budget = lighting ? s.interval_readbacks < 128 && s.lighting_readbacks < 8 :
            s.interval_readbacks < 120;
        if (bytes.empty() && budget) {
            auto* buffer = static_cast<ID3D11Buffer*>(buffers[slot]);
            D3D11_BUFFER_DESC desc{}; buffer->GetDesc(&desc);
            if (desc.ByteWidth <= 8192 && desc.ByteWidth) {
                ++s.readbacks; ++s.interval_readbacks; bytes.resize(desc.ByteWidth);
                if (lighting) ++s.lighting_readbacks;
                ComPtr<ID3D11Device> device; context->GetDevice(&device);
                fallback = true;
                if (rsf_read_constant_buffer(device.Get(), context, buffer, bytes.data(), desc.ByteWidth) != RSF_CONSTANT_BUFFER_OK) {
                    bytes.clear(); ++s.readback_failed;
                }
            }
        }
        const uint32_t id = bytes.empty() ? 0 : save_blob(std::move(bytes));
        if (!first) out << ','; first = false;
        out << "{\"slot\":" << slot << ",\"buffer\":" << uint64_t(uintptr_t(buffers[slot]))
            << ",\"blob\":" << id << ",\"upload_serial\":" << serial
            << ",\"readback\":" << (fallback ? 1 : 0) << '}';
    }
    out << ']';
}
uint32_t shader_hash(void* pointer, uint32_t stage)
{
    auto& s = state(); std::lock_guard<std::mutex> lock(s.guard);
    for (const auto& shader : s.shaders)
        if (shader.pointer == pointer && shader.stage == stage) return shader.hash;
    return 0;
}
uint32_t shader_constants(void* pointer, uint32_t stage)
{
    auto& s = state(); std::lock_guard<std::mutex> lock(s.guard);
    for (const auto& shader : s.shaders)
        if (shader.pointer == pointer && shader.stage == stage) return shader.constant_mask;
    return 0x3fffu;
}
uint32_t declared_constants(const void* bytes, uint32_t size)
{
    // UE strips reflection data. DXBC declarations still name the actual CB slots consumed.
    HMODULE module = LoadLibraryExW(L"d3dcompiler_47.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) return 0x3fffu;
    auto disassemble = reinterpret_cast<decltype(&D3DDisassemble)>(
        reinterpret_cast<void*>(GetProcAddress(module, "D3DDisassemble")));
    ComPtr<ID3DBlob> code;
    uint32_t mask = 0x3fffu;
    if (disassemble && SUCCEEDED(disassemble(bytes, size, 0, nullptr, &code)) && code) {
        mask = 0;
        const std::string text(static_cast<const char*>(code->GetBufferPointer()), code->GetBufferSize());
        constexpr char declaration[] = "dcl_constantbuffer CB";
        size_t offset = 0;
        while ((offset = text.find(declaration, offset)) != std::string::npos) {
            offset += sizeof(declaration) - 1;
            const auto slot = std::strtoul(text.c_str() + offset, nullptr, 10);
            if (slot < 14) mask |= 1u << slot;
        }
    }
    code.Reset(); FreeLibrary(module);
    return mask;
}
void resource(std::ostream& out, ID3D11Resource* r)
{
    D3D11_RESOURCE_DIMENSION dimension{}; r->GetType(&dimension);
    out << "{\"resource\":" << uint64_t(uintptr_t(r)) << ",\"dimension\":" << uint32_t(dimension);
    if (dimension == D3D11_RESOURCE_DIMENSION_TEXTURE2D) {
        D3D11_TEXTURE2D_DESC d{}; static_cast<ID3D11Texture2D*>(r)->GetDesc(&d);
        out << ",\"width\":" << d.Width << ",\"height\":" << d.Height << ",\"format\":" << uint32_t(d.Format);
    } else if (dimension == D3D11_RESOURCE_DIMENSION_TEXTURE3D) {
        D3D11_TEXTURE3D_DESC d{}; static_cast<ID3D11Texture3D*>(r)->GetDesc(&d);
        out << ",\"width\":" << d.Width << ",\"height\":" << d.Height << ",\"depth\":" << d.Depth << ",\"format\":" << uint32_t(d.Format);
    }
    out << '}';
}
void record_draw(std::string text)
{
    auto& s = state(); std::lock_guard<std::mutex> lock(s.guard);
    if (!s.active.load()) return;
    if (s.draws.size() < max_draws) s.draws.push_back(std::move(text)); else ++s.draw_dropped;
}
const char* colour_stage(uint32_t hash)
{
    // Captured AC7 producers, mapped through original DXBC and the RenoDX AC7 reference.
    // These names select diagnostics only and never change rendering decisions.
    switch (hash) {
    case 0x6bcbd787u: case 0x5fb31417u: return "tonemap";
    case 0xa5c9147bu: return "scene_effect";
    case 0xb2f7719du: return "hud";
    case 0x2136e125u: case 0xa7441be5u: return "composite";
    case cloud_composite_shader: return "cloud_composite_after";
    default: return nullptr;
    }
}
void colour_snapshot(std::ostream& out, ID3D11DeviceContext* context, ID3D11Resource* resource,
                     const char* role, bool lighting = false)
{
    auto& s = state();
    // Eight reads at each sampled interval fit in twenty-four reserved slots. The total
    // remains 72, including the existing cloud and post-processing observations.
    if (!role || !resource || s.colour_readbacks >= (lighting ? 72u : 48u)) return;
    ComPtr<ID3D11Texture2D> texture;
    if (FAILED(resource->QueryInterface(IID_PPV_ARGS(&texture)))) return;
    char name[96]{};
    std::snprintf(name, sizeof(name), "f%03u_stage_%s_%03u", s.interval.load(), role, s.colour_readbacks++);
    const std::string path = s.prefix + "\\" + name;
    ComPtr<ID3D11Device> device; context->GetDevice(&device);
    rsf_texture_dump_options options{}; options.struct_size = sizeof(options);
    options.abi_version = RSF_TEXTURE_DUMP_ABI_VERSION; options.output_prefix_utf8 = path.c_str();
    const auto result = rsf_dump_texture_bytes(device.Get(), context, texture.Get(), &options);
    if (result != RSF_TEXTURE_OK) ++s.colour_readback_failed;
    out << ",\"snapshot\":\"" << name << "\",\"snapshot_result\":" << result;
}
void lighting_before(ID3D11DeviceContext* context, const rsf_frame_tap_target_draw& draw, uint32_t hash)
{
    auto& s = state();
    const auto interval = s.interval.load();
    if (s.lighting_interval == interval) return;
    s.lighting_interval = interval; s.lighting_after_pending = true;
    std::ostringstream out;
    out << "{\"kind\":\"contact_light_before\",\"qpc\":" << tick()
        << ",\"interval\":" << interval << ",\"thread\":" << GetCurrentThreadId()
        << ",\"ps_hash\":" << hash << ",\"colour\":{\"view\":";
    resource(out, static_cast<ID3D11Resource*>(draw.render_target));
    colour_snapshot(out, context, static_cast<ID3D11Resource*>(draw.render_target), "contact_light_before", true);
    out << "},\"inputs\":[";
    ID3D11ShaderResourceView* views[6]{}; context->PSGetShaderResources(0, 6, views);
    ComPtr<ID3D11ShaderResourceView> retained[6];
    for (uint32_t i = 0; i < 6; ++i) retained[i].Attach(views[i]);
    constexpr const char* roles[]{"contact_ps_input_0", "contact_ps_input_1", "contact_ps_input_2",
        "contact_light_mask", "contact_scene_depth", "contact_static_shadow"};
    bool first = true;
    for (uint32_t slot = 0; slot < 6; ++slot) {
        if (!retained[slot]) continue;
        ComPtr<ID3D11Resource> texture; retained[slot]->GetResource(&texture);
        if (!first) out << ','; first = false;
        out << "{\"slot\":" << slot << ",\"view\":"; resource(out, texture.Get());
        colour_snapshot(out, context, texture.Get(), roles[slot], true); out << '}';
    }
    out << "],\"samplers\":[";
    ID3D11SamplerState* samplers[6]{}; context->PSGetSamplers(0, 6, samplers);
    first = true;
    for (uint32_t slot = 0; slot < 6; ++slot) if (samplers[slot]) {
        D3D11_SAMPLER_DESC sampler{}; samplers[slot]->GetDesc(&sampler); samplers[slot]->Release();
        if (!first) out << ','; first = false;
        out << "{\"slot\":" << slot << ",\"filter\":" << uint32_t(sampler.Filter)
            << ",\"address\":[" << uint32_t(sampler.AddressU) << ',' << uint32_t(sampler.AddressV) << ',' << uint32_t(sampler.AddressW)
            << "],\"comparison\":" << uint32_t(sampler.ComparisonFunc) << ",\"lod\":[" << sampler.MinLOD << ',' << sampler.MaxLOD << "]}";
    }
    out << "],\"ps_cb\":";
    // Read the two live shader inputs before the light executes, using the reserved quota.
    ID3D11Buffer* raw[14]{}; context->PSGetConstantBuffers(0, 2, raw);
    ComPtr<ID3D11Buffer> retained_buffers[2]; void* buffers[14]{};
    for (uint32_t i = 0; i < 2; ++i) { retained_buffers[i].Attach(raw[i]); buffers[i] = raw[i]; }
    constants(out, context, buffers, 3u, true);
    queued_scope(out); out << '}'; record_draw(out.str());
}
void before_draw_capture(void*, const rsf_frame_tap_target_draw* d) try
{
    auto& s = state();
    if (!s.active.load() || d->target_width < 400 || d->target_height < 200) return;
    const auto ps = shader_hash(d->pixel_shader, 1);
    auto* c = static_cast<ID3D11DeviceContext*>(d->context);
    if (ps == contact_light_shader || ps == contact_light_phased_shader) { lighting_before(c, *d, ps); return; }
    if (ps != cloud_composite_shader) return;
    std::ostringstream out;
    out << "{\"kind\":\"cloud_composite_before\",\"qpc\":" << tick()
        << ",\"interval\":" << s.interval.load() << ",\"thread\":" << GetCurrentThreadId()
        << ",\"target\":" << uint64_t(uintptr_t(d->render_target))
        << ",\"draw_index\":" << d->draw_index << ",\"ps_hash\":" << cloud_composite_shader
        << ",\"colour\":{\"view\":";
    resource(out, static_cast<ID3D11Resource*>(d->render_target));
    colour_snapshot(out, c, static_cast<ID3D11Resource*>(d->render_target), "cloud_composite_before");
    out << "},\"loss\":";
    ID3D11RenderTargetView* targets[2]{}; c->OMGetRenderTargets(2, targets, nullptr);
    ComPtr<ID3D11RenderTargetView> colour_target, loss_target;
    colour_target.Attach(targets[0]); loss_target.Attach(targets[1]);
    if (loss_target) {
        ComPtr<ID3D11Resource> texture; loss_target->GetResource(&texture);
        out << "{\"view\":"; resource(out, texture.Get());
        colour_snapshot(out, c, texture.Get(), "cloud_loss_before"); out << '}';
    } else out << "null";
    out << ",\"inputs\":[";
    ID3D11ShaderResourceView* views[4]{}; c->PSGetShaderResources(0, 4, views);
    ComPtr<ID3D11ShaderResourceView> retained[4];
    for (uint32_t slot = 0; slot < 4; ++slot) retained[slot].Attach(views[slot]);
    constexpr const char* roles[]{"cloud_near", "cloud_far", "cloud_near_far_depth", "cloud_scene_depth"};
    bool first = true;
    for (uint32_t slot = 0; slot < 4; ++slot) {
        if (!retained[slot]) continue;
        ComPtr<ID3D11Resource> texture; retained[slot]->GetResource(&texture);
        if (!first) out << ','; first = false;
        out << "{\"slot\":" << slot << ",\"view\":"; resource(out, texture.Get());
        colour_snapshot(out, c, texture.Get(), roles[slot]); out << '}';
    }
    out << "]}"; record_draw(out.str());
}
catch (...) { capture_failed(); }
void draw_capture(void*, const rsf_frame_tap_target_draw* d) try
{
    auto& s = state();
    if (!s.active.load()) return;
    { std::lock_guard<std::mutex> lock(s.guard); if (s.draws.size() >= max_draws) { ++s.draw_dropped; return; } }
    auto* c = static_cast<ID3D11DeviceContext*>(d->context);
    const uint32_t ps = shader_hash(d->pixel_shader, 1);
    const bool light_after = (ps == contact_light_shader || ps == contact_light_phased_shader) && s.lighting_after_pending;
    if (light_after) s.lighting_after_pending = false;
    ComPtr<ID3D11GeometryShader> gs; ComPtr<ID3D11HullShader> hs; ComPtr<ID3D11DomainShader> ds;
    c->GSGetShader(&gs, nullptr, nullptr); c->HSGetShader(&hs, nullptr, nullptr); c->DSGetShader(&ds, nullptr, nullptr);
    std::ostringstream out; out.precision(9);
    out << "{\"kind\":\"draw\",\"qpc\":" << tick() << ",\"interval\":" << s.interval.load()
        << ",\"thread\":" << GetCurrentThreadId() << ",\"native_scope\":" << current_root
        << ",\"draw_index\":" << d->draw_index
        << ",\"native_view\":" << uint64_t(uintptr_t(current_postprocess_view))
        << ",\"target\":" << uint64_t(uintptr_t(d->render_target)) << ",\"format\":" << d->target_format
        << ",\"extent\":[" << d->target_width << ',' << d->target_height << "],\"targets\":" << d->target_count
        << ",\"depth_bound\":" << d->depth_bound << ",\"indexed\":" << d->indexed << ",\"elements\":" << d->element_count
        << ",\"vs_hash\":" << shader_hash(d->vertex_shader, 0) << ",\"ps_hash\":" << ps
        << ",\"gs_hash\":" << shader_hash(gs.Get(), 2) << ",\"hs_hash\":" << shader_hash(hs.Get(), 3)
        << ",\"ds_hash\":" << shader_hash(ds.Get(), 4)
        << ",\"layout\":" << uint64_t(uintptr_t(d->input_layout)) << ",\"topology\":" << d->topology
        << ",\"stride\":" << d->vertex_stride << ",\"viewport\":[" << d->viewport_x << ',' << d->viewport_y
        << ',' << d->viewport_width << ',' << d->viewport_height << "],\"inputs\":[";
    for (uint32_t i = 0; i < d->input_count; ++i) {
        if (i) out << ',';
        const auto& input = d->inputs[i];
        out << "{\"slot\":" << input.slot << ",\"resource\":" << uint64_t(uintptr_t(input.texture))
            << ",\"width\":" << input.width << ",\"height\":" << input.height << ",\"format\":" << input.format << '}';
    }
    out << "],\"ps_resources\":[";
    ID3D11ShaderResourceView* views[128]{}; c->PSGetShaderResources(0, 128, views);
    bool first = true;
    for (uint32_t i = 0; i < 128; ++i) {
        if (!views[i]) continue;
        if (!first) out << ','; first = false;
        ComPtr<ID3D11Resource> r; views[i]->GetResource(&r);
        out << "{\"slot\":" << i << ",\"view\":"; resource(out, r.Get()); out << '}';
        views[i]->Release();
    }
    out << "],\"vs_resources\":[";
    c->VSGetShaderResources(0, 128, views); first = true;
    for (uint32_t i = 0; i < 128; ++i) {
        if (!views[i]) continue;
        ComPtr<ID3D11Resource> r; views[i]->GetResource(&r);
        if (!first) out << ','; first = false;
        out << "{\"slot\":" << i << ",\"view\":"; resource(out, r.Get()); out << '}';
        views[i]->Release();
    }
    out << "],\"render_targets\":[";
    ID3D11RenderTargetView* targets[8]{}; ComPtr<ID3D11DepthStencilView> depth;
    c->OMGetRenderTargets(8, targets, &depth); first = true;
    for (uint32_t i = 0; i < 8; ++i) {
        if (!targets[i]) continue;
        if (!first) out << ','; first = false;
        ComPtr<ID3D11Resource> r; targets[i]->GetResource(&r);
        out << "{\"slot\":" << i << ",\"view\":"; resource(out, r.Get());
        if (i == 0 && d->target_width >= 400 && d->target_height >= 200)
            colour_snapshot(out, c, r.Get(), colour_stage(ps));
        if (i == 0 && light_after)
            colour_snapshot(out, c, r.Get(), "contact_light_after", true);
        if (i == 1 && ps == cloud_composite_shader && d->target_width >= 400 && d->target_height >= 200)
            colour_snapshot(out, c, r.Get(), "cloud_loss_after");
        out << '}';
        targets[i]->Release();
    }
    out << "],\"depth\":";
    if (depth) { ComPtr<ID3D11Resource> r; depth->GetResource(&r); resource(out, r.Get()); } else out << "null";
    out << ",\"vs_cb\":"; if (d->vertex_shader) constants(out, c, d->vertex_constants, shader_constants(d->vertex_shader, 0)); else out << "[]";
    out << ",\"ps_cb\":"; if (d->pixel_shader) constants(out, c, d->pixel_constants, shader_constants(d->pixel_shader, 1)); else out << "[]";
    out << ",\"gs_cb\":"; if (gs) constants(out, c, d->geometry_constants, shader_constants(gs.Get(), 2)); else out << "[]";
    out << ",\"hs_cb\":"; if (hs) constants(out, c, d->hull_constants, shader_constants(hs.Get(), 3)); else out << "[]";
    out << ",\"ds_cb\":"; if (ds) constants(out, c, d->domain_constants, shader_constants(ds.Get(), 4)); else out << "[]";
    queued_scope(out);
    out << '}';
    record_draw(out.str());
}
catch (...) { capture_failed(); }
void compute_capture(void*, void* context, uint32_t x, uint32_t y, uint32_t z, void* indirect, uint32_t offset) try
{
    auto& s = state(); if (!s.active.load()) return;
    { std::lock_guard<std::mutex> lock(s.guard); if (s.draws.size() >= max_draws) { ++s.draw_dropped; return; } }
    auto* c = static_cast<ID3D11DeviceContext*>(context);
    ComPtr<ID3D11ComputeShader> shader; c->CSGetShader(&shader, nullptr, nullptr);
    std::ostringstream out;
    out << "{\"kind\":\"dispatch\",\"qpc\":" << tick() << ",\"interval\":" << s.interval.load()
        << ",\"thread\":" << GetCurrentThreadId() << ",\"native_scope\":" << current_root
        << ",\"native_view\":" << uint64_t(uintptr_t(current_postprocess_view))
        << ",\"groups\":[" << x << ',' << y << ',' << z << "],\"indirect\":" << uint64_t(uintptr_t(indirect))
        << ",\"offset\":" << offset << ",\"cs_hash\":" << shader_hash(shader.Get(), 5) << ",\"inputs\":[";
    ID3D11ShaderResourceView* views[128]{}; c->CSGetShaderResources(0, 128, views);
    bool first = true;
    for (uint32_t i = 0; i < 128; ++i) {
        if (!views[i]) continue;
        if (!first) out << ','; first = false;
        ComPtr<ID3D11Resource> r; views[i]->GetResource(&r);
        out << "{\"slot\":" << i << ",\"view\":"; resource(out, r.Get()); out << '}';
        views[i]->Release();
    }
    out << "],\"outputs\":[";
    ID3D11UnorderedAccessView* outputs[8]{}; c->CSGetUnorderedAccessViews(0, 8, outputs); first = true;
    for (uint32_t i = 0; i < 8; ++i) {
        if (!outputs[i]) continue;
        if (!first) out << ','; first = false;
        ComPtr<ID3D11Resource> r; outputs[i]->GetResource(&r);
        out << "{\"slot\":" << i << ",\"view\":"; resource(out, r.Get()); out << '}';
        outputs[i]->Release();
    }
    out << "],\"cs_cb\":";
    ID3D11Buffer* cb[14]{}; void* borrowed[14]{}; c->CSGetConstantBuffers(0, 14, cb);
    for (uint32_t i = 0; i < 14; ++i) borrowed[i] = cb[i];
    constants(out, c, borrowed, shader_constants(shader.Get(), 5));
    for (auto* b : cb) if (b) b->Release();
    queued_scope(out);
    out << '}'; record_draw(out.str());
}
catch (...) { capture_failed(); }

bool write_file(const std::string& name, const void* data, size_t bytes)
{
    FILE* f = std::fopen(name.c_str(), "wb");
    if (!f) return false;
    const bool ok = std::fwrite(data, 1, bytes, f) == bytes;
    return std::fclose(f) == 0 && ok;
}
std::string roots_json(const std::vector<RootRecord>& records)
{
    std::ostringstream roots;
    for (const auto& r : records) {
        roots << "{\"kind\":\"" << r.kind << "\",\"id\":" << r.id << ",\"parent\":" << r.parent
            << ",\"qpc\":" << r.qpc << ",\"thread\":" << r.thread << ",\"interval\":" << r.interval
            << ",\"owner\":" << r.owner << ",\"command_list\":" << r.command_list
            << ",\"view\":" << r.view << ",\"view_blob\":" << r.view_blob
            << ",\"context\":" << r.context << ",\"input_node\":" << r.input_node
            << ",\"output_node\":" << r.output_node << ",\"velocity_node\":" << r.velocity_node
            << ",\"vtable\":" << r.vtable << ",\"node_blob\":" << r.node_blob
            << ",\"family\":" << r.family << ",\"native_frame\":" << r.native_frame
            << ",\"process_fn\":" << r.process_fn << ",\"desc_fn\":" << r.desc_fn
            << ",\"output_id\":" << r.output_id << ",\"output_name\":\"" << r.output_name
            << "\",\"output_valid\":" << r.output_valid << ",\"output_extent\":[" << r.width << ',' << r.height
            << "],\"output_format\":" << r.format << ",\"pooled_target\":" << r.pooled_target
            << ",\"queue_begin\":" << r.queue_begin << ",\"queue_end\":" << r.queue_end
            << ",\"queue_uid\":" << r.queue_uid << ",\"commands\":" << r.commands
            << ",\"velocity_reference\":" << r.velocity_reference << ",\"velocity_rhi\":" << r.velocity_rhi
            << ",\"execution_scope\":" << r.execution_scope << ",\"native_role\":" << r.native_role
            << ",\"color_input\":" << r.color_input << ",\"color_output\":" << r.color_output
            << ",\"depth\":" << r.depth << ",\"motion\":" << r.motion
            << ",\"fields_valid\":" << r.fields_valid << ",\"draw_size\":[" << r.draw_size[0] << ',' << r.draw_size[1]
            << "],\"widget\":" << r.widget << ",\"target\":" << r.target
            << ",\"downsample\":" << r.downsample << ",\"blur_x\":" << r.blur_x
            << ",\"blur_y\":" << r.blur_y << ",\"glow\":" << r.glow << "}\n";
    }
    return roots.str();
}
void checkpoint_native() noexcept
{
    try {
        auto& s = state();
        std::string prefix;
        std::vector<RootRecord> roots;
        std::vector<std::vector<unsigned char>> blobs;
        {
            std::lock_guard<std::mutex> lock(s.guard);
            if (!s.active.load()) return;
            prefix = s.prefix; roots = s.roots; blobs = s.blobs;
        }
        const auto text = roots_json(roots);
        bool ok = write_file(prefix + "\\native.partial.jsonl", text.data(), text.size());
        for (size_t i = 0; i < blobs.size(); ++i) {
            const auto& b = blobs[i];
            ok = write_file(prefix + "\\cb_" + std::to_string(i + 1) + ".bin", b.data(), b.size()) && ok;
        }
        if (!ok) say("motion capture: native checkpoint could not be fully written");
    } catch (...) { capture_failed(); }
}
void finish()
{
    auto& s = state();
    s.active.store(false, std::memory_order_release);
    rsf_frame_tap_set_research_callbacks(nullptr, nullptr, nullptr);
    std::lock_guard<std::mutex> lock(s.guard);
    std::ostringstream engine; engine.precision(9);
    for (const auto& e : s.engine) {
        const auto& f = e.facts;
        engine << "{\"qpc\":" << e.qpc << ",\"interval\":" << e.interval << ",\"thread\":" << e.thread
            << ",\"primitive\":" << e.primitive << ",\"proxy\":" << e.proxy << ",\"view\":" << e.view
            << ",\"component\":" << e.component << ",\"index\":" << e.index << ",\"accepted\":" << f.accepted
            << ",\"reason\":\"" << rsf_ac7_velocity_reason(&f) << "\",\"mobility\":" << f.mobility
            << ",\"relevance\":" << f.relevance << ",\"fields_valid\":" << f.fields_valid
            << ",\"visible\":" << f.visible << ",\"camera_cut\":" << f.camera_cut
            << ",\"always_velocity\":" << f.always_velocity
            << ",\"has_velocity_called\":" << f.has_velocity_called << ",\"has_velocity\":" << f.has_velocity
            << ",\"selection_metrics\":";
        const float metrics[] = {f.radius, f.distance_squared, f.lod_factor, f.minimum_size};
        floats(engine, metrics);
        engine << ",\"camera\":"; floats(engine, e.camera);
        engine << ",\"origin\":"; floats(engine, e.origin);
        engine << ",\"current\":"; floats(engine, e.current);
        engine << ",\"history_checked\":" << f.history_checked << ",\"history_found\":" << f.history_found
            << ",\"previous\":"; floats(engine, e.previous); engine << "}\n";
    }
    const std::string engine_text = engine.str();
    bool ok = write_file(s.prefix + "\\engine.jsonl", engine_text.data(), engine_text.size());
    const std::string roots_text = roots_json(s.roots);
    ok = write_file(s.prefix + "\\native.jsonl", roots_text.data(), roots_text.size()) && ok;
    std::string draw_text; for (const auto& line : s.draws) { draw_text += line; draw_text += '\n'; }
    ok = write_file(s.prefix + "\\draws.jsonl", draw_text.data(), draw_text.size()) && ok;
    for (size_t i = 0; i < s.blobs.size(); ++i) {
        const auto& b = s.blobs[i];
        ok = write_file(s.prefix + "\\cb_" + std::to_string(i + 1) + ".bin", b.data(), b.size()) && ok;
    }
    for (size_t i = 0; i < s.shaders.size(); ++i) {
        const auto& shader = s.shaders[i];
        const std::string path = s.prefix + "\\shader_" + std::to_string(shader.stage) + "_" +
                                 std::to_string(i + 1) + "_" + std::to_string(shader.hash) + ".dxbc";
        ok = write_file(path, shader.data.data(), shader.data.size()) && ok;
    }
    LARGE_INTEGER frequency{}; QueryPerformanceFrequency(&frequency);
    std::ostringstream summary;
    summary << "{\"schema\":1,\"qpc_frequency\":" << frequency.QuadPart << ",\"engine_hooks\":" << s.installed.load()
        << ",\"renderer_root_hooks\":" << s.roots_installed.load() << ",\"root_records\":" << s.roots.size()
        << ",\"root_dropped\":" << s.roots_dropped
        << ",\"command_associations\":" << s.commands.size() << ",\"command_associations_dropped\":" << s.commands_dropped
        << ",\"intervals\":60,\"engine_records\":" << s.engine.size() << ",\"draw_records\":" << s.draws.size()
        << ",\"engine_dropped\":" << s.engine_dropped << ",\"draw_dropped\":" << s.draw_dropped
        << ",\"blob_dropped\":" << s.blob_dropped << ",\"shader_creation_dropped\":" << s.shaders_dropped
        << ",\"readbacks\":" << s.readbacks << ",\"readback_failures\":" << s.readback_failed
        << ",\"colour_readbacks\":" << s.colour_readbacks << ",\"colour_readback_failures\":" << s.colour_readback_failed
        << ",\"capture_failures\":" << s.failures.load()
        << ",\"files_written\":" << (ok ? "true" : "false")
        << ",\"timing\":\"present intervals and QPC; CPU selection may precede GPU execution\""
        << ",\"draw_state\":\"shadow draw facts are game-requested; live resources and targets are post-draw bindings; temporary constant-buffer overrides restored\""
        << ",\"gpu_context\":\"installed immediate context; deferred command lists not decomposed\""
        << ",\"native_scope\":\"thread-local engine call scope only; zero outside it. CPU scopes are not GPU frame identity. view_blob uses cb_<id>.bin and AC7 stride 0x27c0\""
        << ",\"queued_scope\":\"candidate graph-to-RHI command match by pointer and execute thunk, bounded to sampled native calls; validates neither GPU completion nor Present identity\""
        << ",\"selection_metric_order\":[\"bounds_radius\",\"distance_squared\",\"lod_factor\",\"minimum_size\"]}\n";
    const std::string summary_text = summary.str();
    ok = write_file(s.prefix + "\\session.json", summary_text.data(), summary_text.size()) && ok;
    say((std::string("motion capture finished: ") + s.prefix + (ok ? "" : " (some writes failed)")).c_str());
}
} // namespace

extern "C" void rsf_ac7_motion_capture_configure(const char* directory, rsf_ac7_capture_log_fn log, void* user) try
{
    auto& s = state(); s.directory = directory ? directory : ""; s.log = log; s.user = user;
    s.configured = !s.directory.empty();
    if (s.configured) say("motion capture configured: F9 records 60 intervals, sampled at 0/30/59");
}
catch (...) { capture_failed(); }
extern "C" int rsf_ac7_motion_capture_install(void)
{
    auto& s = state(); if (!s.configured || s.installed) return s.installed ? 1 : 0;
    auto* base = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr));
    const MH_STATUS init = MH_Initialize();
    if (init != MH_OK && init != MH_ERROR_ALREADY_INITIALIZED) { say("motion capture: MinHook unavailable"); return 0; }
    install_roots(base);
    const unsigned char should[] = {0x40,0x53,0x48,0x83,0xec,0x20,0x4c,0x8b,0xda,0x48,0x8b,0xd9,0x45,0x84,0xc0};
    const unsigned char has[] = {0x40,0x53,0x48,0x83,0xec,0x60,0x80,0xb9,0x34,0x0c,0,0,0};
    const unsigned char history[] = {0x40,0x53,0x48,0x83,0xec,0x20,0x49,0x8b,0xd8,0x48,0x85,0xd2,0x74,0x3a};
    unsigned char actual[sizeof(should)]{};
    if (!base || !copy_memory(actual, base + 0x10fdc00, sizeof(should)) || std::memcmp(actual, should, sizeof(should)) ||
        !copy_memory(actual, base + 0x1183820, sizeof(has)) || std::memcmp(actual, has, sizeof(has)) ||
        !copy_memory(actual, base + 0x10ee670, sizeof(history)) || std::memcmp(actual, history, sizeof(history)) ||
        !copy_memory(actual, base + 0x10fdd09, 5) || std::memcmp(actual, "\x0f\x2f\xdd\x76\x38", 5)) {
        say("motion capture engine hooks refused: expected AC7 bytes differ"); return 0;
    }
    void* detours[] = {reinterpret_cast<void*>(&hooked_should), reinterpret_cast<void*>(&hooked_has), reinterpret_cast<void*>(&hooked_history)};
    void** originals[] = {reinterpret_cast<void**>(&original_should), reinterpret_cast<void**>(&original_has), reinterpret_cast<void**>(&original_history)};
    const uint32_t rvas[] = {0x10fdc00, 0x1183820, 0x10ee670};
    uint32_t made = 0;
    for (; made < 3; ++made) {
        hook_targets[made] = base + rvas[made];
        if (MH_CreateHook(hook_targets[made], detours[made], originals[made]) != MH_OK) break;
    }
    if (made == 3) {
        bool enabled = true;
        for (uint32_t i = 0; i < 3; ++i) enabled = MH_EnableHook(hook_targets[i]) == MH_OK && enabled;
        if (enabled) { s.installed = true; say("motion capture engine hooks installed; original decisions preserved"); return 1; }
    }
    for (uint32_t i = 0; i < made; ++i) { MH_DisableHook(hook_targets[i]); MH_RemoveHook(hook_targets[i]); }
    say("motion capture engine hooks failed and were rolled back"); return 0;
}
extern "C" void rsf_ac7_motion_capture_shutdown(void)
{
    auto& s = state(); s.active.store(false);
    rsf_frame_tap_set_research_callbacks(nullptr, nullptr, nullptr);
    if (s.installed) for (auto* p : hook_targets) { MH_DisableHook(p); MH_RemoveHook(p); }
    if (s.roots_installed) for (auto* p : root_targets) { MH_DisableHook(p); MH_RemoveHook(p); }
    s.roots_installed = false;
    s.installed = false;
}
extern "C" void rsf_ac7_motion_capture_shader(void* shader, uint32_t stage, const void* bytes, uint32_t size) try
{
    auto& s = state(); if (!s.configured || !shader || !bytes || !size || size > 1024u * 1024u) return;
    const auto mask = declared_constants(bytes, size);
    std::lock_guard<std::mutex> lock(s.guard);
    for (auto i = s.shaders.begin(); i != s.shaders.end(); ++i) {
        if (i->pointer == shader && i->stage == stage) { s.shader_bytes -= i->data.size(); s.shaders.erase(i); break; }
    }
    if (s.shaders.size() >= max_blobs || s.shader_bytes + size > shader_budget) { ++s.shaders_dropped; return; }
    Shader item; item.pointer = shader; item.stage = stage; item.hash = rsf_ui_shader_hash(bytes, size);
    item.constant_mask = mask;
    item.data.assign(static_cast<const unsigned char*>(bytes), static_cast<const unsigned char*>(bytes) + size);
    s.shader_bytes += size; s.shaders.push_back(std::move(item));
}
catch (...) { capture_failed(); }
extern "C" void rsf_ac7_motion_capture_buffer(void*, void* buffer, const void* initial, uint32_t bytes, uint32_t bind_flags) try
{
    auto& s = state(); if (!s.configured) return;
    { std::lock_guard<std::mutex> lock(s.guard); s.buffers.erase(buffer); }
    if (initial && (bind_flags & D3D11_BIND_CONSTANT_BUFFER)) rsf_ac7_motion_capture_upload(buffer, initial, bytes);
}
catch (...) { capture_failed(); }
extern "C" void rsf_ac7_motion_capture_upload(void* buffer, const void* bytes, uint32_t size) try
{
    auto& s = state(); if (!s.configured || !buffer || !bytes || !size || size > 8192) return;
    std::lock_guard<std::mutex> lock(s.guard);
    if (!s.buffers.contains(buffer) && s.buffers.size() >= 2048) return;
    auto& snapshot = s.buffers[buffer]; snapshot.serial = ++s.upload_serial;
    snapshot.data.assign(static_cast<const unsigned char*>(bytes), static_cast<const unsigned char*>(bytes) + size);
}
catch (...) { capture_failed(); }
extern "C" void rsf_ac7_motion_capture_native_owner(uint32_t enabled)
{ native_owner.store(enabled != 0); }
extern "C" void rsf_ac7_motion_capture_native_pass(const rsf_game_render_pass* pass, uint32_t begin) try
{
    if (!pass || pass->struct_size < sizeof(*pass)) { execution_pass = {}; return; }
    execution_pass = *pass;
    auto& s = state(); if (!s.active.load()) return;
    const auto interval = s.interval.load(); if (interval != 0 && interval != 30 && interval != 59) return;
    auto r = root_record(begin ? "rhi_scope_begin" : "rhi_scope_restore", nullptr);
    r.owner = pass->pass_key; r.view = pass->view_key; r.family = pass->family_key;
    r.native_frame = uint32_t(pass->native_frame); r.execution_scope = pass->scope_id; r.native_role = pass->role;
    r.color_input = uint64_t(uintptr_t(pass->color_input)); r.color_output = uint64_t(uintptr_t(pass->color_output));
    r.depth = uint64_t(uintptr_t(pass->depth)); r.motion = uint64_t(uintptr_t(pass->motion));
    r.width = uint32_t(pass->output_rect[2]); r.height = uint32_t(pass->output_rect[3]); r.fields_valid = pass->scope_id != 0;
    record_root(r);
}
catch (...) { capture_failed(); }
extern "C" void rsf_ac7_motion_capture_request(void)
{ state().request.store(true); }
extern "C" int rsf_ac7_motion_capture_present(void* swapchain, char* prefix, uint32_t capacity) try
{
    auto& s = state(); if (!s.configured || !swapchain || !prefix || !capacity) return 0;
    prefix[0] = '\0'; ++s.presents;
    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(static_cast<IDXGISwapChain*>(swapchain)->GetDesc(&desc))) return 0;
    DWORD process = 0; GetWindowThreadProcessId(GetForegroundWindow(), &process);
    const bool pressed = process == GetCurrentProcessId() && (GetAsyncKeyState(VK_F9) & 0x8000) != 0;
    const bool requested = s.request.exchange(false);
    if ((requested || (pressed && !s.key_down)) && !s.active.load()) {
        SYSTEMTIME time{}; GetLocalTime(&time);
        char name[96]{};
        std::snprintf(name, sizeof(name), "\\motion-%04u%02u%02u-%02u%02u%02u-%lu-%u",
                      time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond,
                      GetCurrentProcessId(), s.session.load() + 1);
        s.prefix = s.directory + name;
        if (CreateDirectoryA(s.prefix.c_str(), nullptr)) {
            std::lock_guard<std::mutex> lock(s.guard);
            s.engine.clear(); s.roots.clear(); s.roots_dropped = 0; s.commands.clear(); s.commands_dropped = 0;
            s.draws.clear(); s.blobs.clear(); s.blob_bytes = 0;
            s.engine_dropped = s.draw_dropped = s.blob_dropped = s.readbacks = s.readback_failed = 0;
            s.readback_interval = UINT32_MAX; s.interval_readbacks = 0;
            s.lighting_readbacks = 0; s.lighting_interval = UINT32_MAX; s.lighting_after_pending = false;
            s.colour_readbacks = s.colour_readback_failed = 0;
            s.start = s.presents; s.session.fetch_add(1); s.interval.store(0); s.active.store(true);
            say((std::string("motion capture armed: ") + s.prefix).c_str());
        } else say("motion capture: output directory could not be created");
    }
    s.key_down = pressed;
    if (!s.active.load()) return 0;
    const auto age = uint32_t(s.presents - s.start);
    if (age >= 60) { finish(); return 0; }
    s.interval.store(age);
    const bool sample = age == 0 || age == 30 || age == 59;
    rsf_frame_tap_set_research_phase_callbacks(sample ? before_draw_capture : nullptr,
        sample ? draw_capture : nullptr, sample ? compute_capture : nullptr, nullptr);
    if (!sample) return 0;
    std::snprintf(prefix, capacity, "%s\\f%03u", s.prefix.c_str(), age);
    return 1;
}
catch (...) { capture_failed(); rsf_frame_tap_set_research_callbacks(nullptr, nullptr, nullptr); return 0; }
