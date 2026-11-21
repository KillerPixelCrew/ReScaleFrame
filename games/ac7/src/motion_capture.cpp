// SPDX-License-Identifier: MIT
#include <rescaleframe/ac7_hooks.h>
#include <rescaleframe/ac7_memory.h>
#include <rescaleframe/ac7_motion_capture.h>
#include <rescaleframe/constant_buffer_read.h>
#include <rescaleframe/frame_tap.h>
#include <rescaleframe/texture_dump.h>
#include "motion_capture_observations.h"
#include "motion_capture_records.h"
#include "motion_capture_session.h"

#include <windows.h>
#include <d3d11.h>
#include <d3dcompiler.h>
#include <dxgi.h>
#include <MinHook.h>
#include <wrl/client.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <sstream>
#include <set>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

using Microsoft::WRL::ComPtr;

namespace {
constexpr uint32_t max_draws = 12000;
constexpr uint32_t max_blobs = 4096;
constexpr size_t blob_budget = 32u * 1024u * 1024u;
constexpr uint32_t cloud_composite_shader = 0x83524e47u;
// Captured directional light variant: screen-space contact ray and stationary IGN.
constexpr uint32_t contact_light_shader = 3619939816u;
constexpr uint32_t contact_light_phased_shader = 4154163049u; // AC7 transform: noise phase and depth quantisation bias

struct Bytes { std::vector<unsigned char> data; uint64_t serial = 0; };
// Counters for the typed graphics effects.
struct SessionCounters {
    uint32_t draw_dropped = 0, blob_dropped = 0;
    uint32_t readbacks = 0, readback_failed = 0;
    uint32_t readback_interval = UINT32_MAX, interval_readbacks = 0;
    uint32_t lighting_readbacks = 0, lighting_interval = UINT32_MAX;
    bool lighting_after_pending = false;
    uint32_t colour_readbacks = 0, colour_readback_failed = 0;
};
struct Capture {
    std::mutex guard;
    std::atomic<bool> installed{false};
    std::atomic<bool> roots_installed{false};
    uint64_t upload_serial = 0;
    std::string directory, prefix;
    rsf_ac7_capture_log_fn log = nullptr;
    void* user = nullptr;
    std::vector<std::string> draws;
    std::vector<std::vector<unsigned char>> blobs;
    std::unordered_map<void*, Bytes> buffers;
    uint32_t geometry_interval = UINT32_MAX;
    std::array<uint32_t,5> geometry_reads{};
    std::array<std::set<std::pair<uint32_t,void*>>,5> geometry_seen;
    size_t blob_bytes = 0;
    SessionCounters counters;
};
Capture& state() { static Capture s; return s; }
std::atomic<bool> native_owner{false};
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

uint64_t tick()
{
    LARGE_INTEGER t{}; QueryPerformanceCounter(&t); return uint64_t(t.QuadPart);
}
void say(const char* message) { auto& s = state(); if (s.log) s.log(s.user, message); }
void capture_failed(MotionCaptureInterval observed) noexcept
{
    rsf_ac7_capture_fail(observed);
    say("motion capture stopped after an allocation or serialization failure; game decisions are unchanged");
}
void capture_failed() noexcept { capture_failed(rsf_ac7_capture_interval()); }

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
    auto& s = state();
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
    try {
        bool recorded = true;
        {
            std::lock_guard<std::mutex> lock(s.guard);
            if (rsf_ac7_capture_current(capture.session))
                recorded = rsf_ac7_capture_record_engine(&event, accepted);
        }
        if (!recorded) capture_failed(capture);
    } catch (...) { capture_failed(capture); }
    return accepted;
}

uint32_t save_blob(MotionCaptureInterval capture, std::vector<unsigned char> bytes)
{
    auto& s = state();
    std::lock_guard<std::mutex> lock(s.guard);
    if (!rsf_ac7_capture_current(capture.session)) return 0;
    for (size_t i = 0; i < s.blobs.size(); ++i)
        if (s.blobs[i] == bytes) return uint32_t(i + 1);
    if (s.blobs.size() >= max_blobs || s.blob_bytes + bytes.size() > blob_budget) {
        ++s.counters.blob_dropped; return 0;
    }
    s.blob_bytes += bytes.size(); s.blobs.push_back(std::move(bytes));
    return uint32_t(s.blobs.size());
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
    try {
        auto& s = state();
        bool recorded;
        {
            std::lock_guard<std::mutex> lock(s.guard);
            if (!rsf_ac7_capture_current(r.session)) return;
            recorded = rsf_ac7_capture_record_root(&r);
        }
        if (!recorded) capture_failed({r.session, r.interval});
    } catch (...) { capture_failed({r.session, r.interval}); }
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
            if (copy_memory(bytes.data(), view, bytes.size())) r.view_blob = save_blob(capture, std::move(bytes));
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
            if (copy_memory(bytes.data(), node, bytes.size())) r.node_blob = save_blob(capture, std::move(bytes));
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
        auto& s = state(); uint64_t link = 0, command = 0;
        uint32_t uid = 0;
        auto* list = reinterpret_cast<void*>(uintptr_t(r.command_list));
        if (!read(list, 8, link) || !read(list, 0x18, uid) ||
            !rsf_ac7_capture_commands_changed(&r, link, uid) || !read(r.queue_begin, 0, command)) return;
        r.queue_end = link;
        while (command) {
            uint64_t next = 0, execute = 0;
            auto* p = reinterpret_cast<void*>(uintptr_t(command));
            if (!read(p, 0, next) || !read(p, 8, execute)) break;
            CommandAdvance advance;
            {
                std::lock_guard<std::mutex> lock(s.guard);
                if (!rsf_ac7_capture_current(r.session)) return;
                advance = rsf_ac7_capture_associate_command(&r, command, execute);
            }
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
void queued_scope(std::ostream& out)
{
    uint64_t command = 0, execute = 0;
    QueuedScope scope;
    if (state().roots_installed && copy_memory(&command, current_rhi_command_address, sizeof(command)) && command &&
        read(command, 8, execute)) {
        auto& s = state(); std::lock_guard<std::mutex> lock(s.guard);
        scope = rsf_ac7_capture_queued_scope(command, execute, true);
    } else scope = rsf_ac7_capture_queued_scope(command, execute, false);
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
    for (uint32_t i = first; i < 6; ++i) {
        if (i == 2 && native_owner.load()) continue;
        root_targets[i] = base + rvas[i];
    }
    if (rsf::ac7::install_hooks(root_targets, detours, originals, 6)) {
        s.roots_installed = true;
        current_rhi_command_address = base + 0x3c78358;
        say("renderer root capture installed: graph/RHI associations, main temporal graph and widget producers; engine arguments unchanged"); return;
    }
    std::fill(std::begin(root_targets), std::end(root_targets), nullptr);
    say("renderer root capture failed and was rolled back");
}
void constants(MotionCaptureInterval capture, std::ostream& out, ID3D11DeviceContext* context, void* const* buffers,
               uint32_t mask = 0x3fffu, bool lighting = false)
{
    auto& s = state();
    out << '['; bool first = true;
    for (uint32_t slot = 0; slot < 14; ++slot) {
        if (!buffers[slot] || !(mask & (1u << slot))) continue;
        std::vector<unsigned char> bytes;
        uint64_t serial = 0;
        bool budget;
        const auto has_budget = [&] {
            return lighting ? s.counters.interval_readbacks < 128 && s.counters.lighting_readbacks < 8 :
                s.counters.interval_readbacks < 120;
        };
        {
            std::lock_guard<std::mutex> lock(s.guard);
            if (rsf_ac7_capture_interval() != capture) break;
            if (s.counters.readback_interval != capture.interval) {
                s.counters.readback_interval = capture.interval;
                s.counters.interval_readbacks = s.counters.lighting_readbacks = 0;
            }
            if (!lighting) {
                const auto found = s.buffers.find(buffers[slot]);
                if (found != s.buffers.end()) { bytes = found->second.data; serial = found->second.serial; }
            }
            // Reserve eight of the existing 128 reads for the verified lighting producer.
            budget = has_budget();
        }
        bool fallback = false;
        if (bytes.empty() && budget) {
            auto* buffer = static_cast<ID3D11Buffer*>(buffers[slot]);
            D3D11_BUFFER_DESC desc{}; buffer->GetDesc(&desc);
            if (desc.ByteWidth <= 8192 && desc.ByteWidth) {
                {
                    std::lock_guard<std::mutex> lock(s.guard);
                    if (rsf_ac7_capture_interval() != capture) break;
                    fallback = has_budget();
                    if (fallback) {
                        ++s.counters.readbacks; ++s.counters.interval_readbacks;
                        if (lighting) ++s.counters.lighting_readbacks;
                    }
                }
            }
            if (fallback) {
                bytes.resize(desc.ByteWidth);
                ComPtr<ID3D11Device> device; context->GetDevice(&device);
                if (rsf_read_constant_buffer(device.Get(), context, buffer, bytes.data(), desc.ByteWidth) != RSF_CONSTANT_BUFFER_OK) {
                    bytes.clear();
                    std::lock_guard<std::mutex> lock(s.guard);
                    if (rsf_ac7_capture_current(capture.session)) ++s.counters.readback_failed;
                }
            }
        }
        const uint32_t id = bytes.empty() ? 0 : save_blob(capture, std::move(bytes));
        if (!first) out << ','; first = false;
        out << "{\"slot\":" << slot << ",\"buffer\":" << uint64_t(uintptr_t(buffers[slot]))
            << ",\"blob\":" << id << ",\"upload_serial\":" << serial
            << ",\"readback\":" << (fallback ? 1 : 0) << '}';
    }
    out << ']';
}
uint32_t shader_hash(void* pointer, uint32_t stage) { return rsf_ac7_capture_shader_facts(pointer, stage).hash; }
uint32_t shader_constants(void* pointer, uint32_t stage) { return rsf_ac7_capture_shader_facts(pointer, stage).constant_mask; }
uint32_t disassemble_constants(const void* bytes, size_t size) noexcept
{
    // UE strips reflection data. DXBC declarations still name the actual CB slots consumed.
    // Loaded once and kept: this runs on every shader creation, possibly under the loader lock.
    static const auto disassemble = [] {
        HMODULE module = LoadLibraryExW(L"d3dcompiler_47.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
        return module ? reinterpret_cast<decltype(&D3DDisassemble)>(
            reinterpret_cast<void*>(GetProcAddress(module, "D3DDisassemble"))) : nullptr;
    }();
    ComPtr<ID3DBlob> code;
    if (disassemble && SUCCEEDED(disassemble(bytes, size, 0, nullptr, &code)) && code) {
        return rsf_ac7_capture_declared_constants(
            static_cast<const unsigned char*>(code->GetBufferPointer()), code->GetBufferSize());
    }
    return rsf_ac7_capture_declared_constants(nullptr, 0);
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
template<class View, size_t N> void bound_views(std::ostream& out, View* (&views)[N])
{
    bool first = true;
    for (size_t i = 0; i < N; ++i) {
        if (!views[i]) continue;
        ComPtr<ID3D11Resource> r; views[i]->GetResource(&r);
        if (!first) out << ','; first = false;
        out << "{\"slot\":" << i << ",\"view\":"; resource(out, r.Get()); out << '}';
        views[i]->Release();
    }
}
void record_draw(MotionCaptureInterval capture, std::string text)
{
    auto& s = state(); std::lock_guard<std::mutex> lock(s.guard);
    if (!rsf_ac7_capture_current(capture.session)) return;
    if (s.draws.size() < max_draws) s.draws.push_back(std::move(text)); else ++s.counters.draw_dropped;
}
bool can_record_draw(MotionCaptureInterval capture)
{
    auto& s = state(); std::lock_guard<std::mutex> lock(s.guard);
    if (!rsf_ac7_capture_current(capture.session)) return false;
    if (s.draws.size() < max_draws) return true;
    ++s.counters.draw_dropped;
    return false;
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
void colour_snapshot(MotionCaptureInterval capture, std::ostream& out, ID3D11DeviceContext* context,
                     ID3D11Resource* resource, const char* role, bool lighting = false)
{
    auto& s = state();
    if (!role || !resource) return;
    // Eight reads at each sampled interval fit in twenty-four reserved slots. The total
    // remains 72, including the existing cloud and post-processing observations.
    const auto budget = lighting ? 72u : 48u;
    {
        std::lock_guard<std::mutex> lock(s.guard);
        if (rsf_ac7_capture_interval() != capture || s.counters.colour_readbacks >= budget) return;
    }
    ComPtr<ID3D11Texture2D> texture;
    if (FAILED(resource->QueryInterface(IID_PPV_ARGS(&texture)))) return;
    char name[96]{};
    std::string path;
    {
        std::lock_guard<std::mutex> lock(s.guard);
        if (rsf_ac7_capture_interval() != capture || s.counters.colour_readbacks >= budget) return;
        std::snprintf(name, sizeof(name), "f%03u_stage_%s_%03u", capture.interval, role, s.counters.colour_readbacks++);
        path = s.prefix + "\\" + name;
    }
    ComPtr<ID3D11Device> device; context->GetDevice(&device);
    rsf_texture_dump_options options{}; options.struct_size = sizeof(options);
    options.abi_version = RSF_TEXTURE_DUMP_ABI_VERSION; options.output_prefix_utf8 = path.c_str();
    const auto result = rsf_dump_texture_bytes(device.Get(), context, texture.Get(), &options);
    if (result != RSF_TEXTURE_OK) {
        std::lock_guard<std::mutex> lock(s.guard);
        if (rsf_ac7_capture_current(capture.session)) ++s.counters.colour_readback_failed;
    }
    out << ",\"snapshot\":\"" << name << "\",\"snapshot_result\":" << result;
}
void lighting_before(MotionCaptureInterval capture, ID3D11DeviceContext* context,
                     const rsf_frame_tap_target_draw& draw, uint32_t hash)
{
    auto& s = state();
    const auto interval = capture.interval;
    {
        std::lock_guard<std::mutex> lock(s.guard);
        if (rsf_ac7_capture_interval() != capture || s.counters.lighting_interval == interval) return;
        s.counters.lighting_interval = interval; s.counters.lighting_after_pending = true;
    }
    std::ostringstream out;
    out << "{\"kind\":\"contact_light_before\",\"qpc\":" << tick()
        << ",\"interval\":" << interval << ",\"thread\":" << GetCurrentThreadId()
        << ",\"ps_hash\":" << hash << ",\"colour\":{\"view\":";
    resource(out, static_cast<ID3D11Resource*>(draw.render_target));
    colour_snapshot(capture, out, context, static_cast<ID3D11Resource*>(draw.render_target), "contact_light_before", true);
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
        colour_snapshot(capture, out, context, texture.Get(), roles[slot], true); out << '}';
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
    constants(capture, out, context, buffers, 3u, true);
    queued_scope(out); out << '}'; record_draw(capture, out.str());
}
void before_draw_capture(MotionCaptureInterval capture, const rsf_frame_tap_target_draw* d) try
{
    if (!capture || d->target_width < 400 || d->target_height < 200) return;
    const auto ps = shader_hash(d->pixel_shader, 1);
    auto* c = static_cast<ID3D11DeviceContext*>(d->context);
    if (ps == contact_light_shader || ps == contact_light_phased_shader) { lighting_before(capture, c, *d, ps); return; }
    if (ps != cloud_composite_shader) return;
    std::ostringstream out;
    out << "{\"kind\":\"cloud_composite_before\",\"qpc\":" << tick()
        << ",\"interval\":" << capture.interval << ",\"thread\":" << GetCurrentThreadId()
        << ",\"target\":" << uint64_t(uintptr_t(d->render_target))
        << ",\"draw_index\":" << d->draw_index << ",\"ps_hash\":" << cloud_composite_shader
        << ",\"colour\":{\"view\":";
    resource(out, static_cast<ID3D11Resource*>(d->render_target));
    colour_snapshot(capture, out, c, static_cast<ID3D11Resource*>(d->render_target), "cloud_composite_before");
    out << "},\"loss\":";
    ID3D11RenderTargetView* targets[2]{}; c->OMGetRenderTargets(2, targets, nullptr);
    ComPtr<ID3D11RenderTargetView> colour_target, loss_target;
    colour_target.Attach(targets[0]); loss_target.Attach(targets[1]);
    if (loss_target) {
        ComPtr<ID3D11Resource> texture; loss_target->GetResource(&texture);
        out << "{\"view\":"; resource(out, texture.Get());
        colour_snapshot(capture, out, c, texture.Get(), "cloud_loss_before"); out << '}';
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
        colour_snapshot(capture, out, c, texture.Get(), roles[slot]); out << '}';
    }
    out << "]}"; record_draw(capture, out.str());
}
catch (...) { capture_failed(capture); }
void draw_capture(MotionCaptureInterval capture, const rsf_frame_tap_target_draw* d) try
{
    if (!capture || !can_record_draw(capture)) return;
    auto* c = static_cast<ID3D11DeviceContext*>(d->context);
    const uint32_t ps = shader_hash(d->pixel_shader, 1);
    bool light_after = false;
    if (ps == contact_light_shader || ps == contact_light_phased_shader) {
        auto& s = state(); std::lock_guard<std::mutex> lock(s.guard);
        if (rsf_ac7_capture_interval() == capture && s.counters.lighting_interval == capture.interval)
            light_after = std::exchange(s.counters.lighting_after_pending, false);
    }
    ComPtr<ID3D11GeometryShader> gs; ComPtr<ID3D11HullShader> hs; ComPtr<ID3D11DomainShader> ds;
    c->GSGetShader(&gs, nullptr, nullptr); c->HSGetShader(&hs, nullptr, nullptr); c->DSGetShader(&ds, nullptr, nullptr);
    std::ostringstream out; out.precision(9);
    const auto scope = rsf_ac7_capture_root_scope();
    out << "{\"kind\":\"draw\",\"qpc\":" << tick() << ",\"interval\":" << capture.interval
        << ",\"thread\":" << GetCurrentThreadId() << ",\"native_scope\":" << scope.id
        << ",\"draw_index\":" << d->draw_index
        << ",\"native_view\":" << scope.view
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
    bound_views(out, views);
    out << "],\"vs_resources\":[";
    c->VSGetShaderResources(0, 128, views);
    bound_views(out, views);
    out << "],\"render_targets\":[";
    ID3D11RenderTargetView* targets[8]{}; ComPtr<ID3D11DepthStencilView> depth;
    c->OMGetRenderTargets(8, targets, &depth); bool first = true;
    for (uint32_t i = 0; i < 8; ++i) {
        if (!targets[i]) continue;
        if (!first) out << ','; first = false;
        ComPtr<ID3D11Resource> r; targets[i]->GetResource(&r);
        out << "{\"slot\":" << i << ",\"view\":"; resource(out, r.Get());
        if (i == 0 && d->target_width >= 400 && d->target_height >= 200)
            colour_snapshot(capture, out, c, r.Get(), colour_stage(ps));
        if (i == 0 && light_after)
            colour_snapshot(capture, out, c, r.Get(), "contact_light_after", true);
        if (i == 1 && ps == cloud_composite_shader && d->target_width >= 400 && d->target_height >= 200)
            colour_snapshot(capture, out, c, r.Get(), "cloud_loss_after");
        out << '}';
        targets[i]->Release();
    }
    out << "],\"depth\":";
    if (depth) { ComPtr<ID3D11Resource> r; depth->GetResource(&r); resource(out, r.Get()); } else out << "null";
    out << ",\"vs_cb\":"; if (d->vertex_shader) constants(capture, out, c, d->vertex_constants, shader_constants(d->vertex_shader, 0)); else out << "[]";
    out << ",\"ps_cb\":"; if (d->pixel_shader) constants(capture, out, c, d->pixel_constants, shader_constants(d->pixel_shader, 1)); else out << "[]";
    out << ",\"gs_cb\":"; if (gs) constants(capture, out, c, d->geometry_constants, shader_constants(gs.Get(), 2)); else out << "[]";
    out << ",\"hs_cb\":"; if (hs) constants(capture, out, c, d->hull_constants, shader_constants(hs.Get(), 3)); else out << "[]";
    out << ",\"ds_cb\":"; if (ds) constants(capture, out, c, d->domain_constants, shader_constants(ds.Get(), 4)); else out << "[]";
    queued_scope(out);
    out << '}';
    record_draw(capture, out.str());
}
catch (...) { capture_failed(capture); }
void compute_capture(MotionCaptureInterval capture, void* context, uint32_t x, uint32_t y, uint32_t z, void* indirect, uint32_t offset) try
{
    if (!capture || !can_record_draw(capture)) return;
    auto* c = static_cast<ID3D11DeviceContext*>(context);
    ComPtr<ID3D11ComputeShader> shader; c->CSGetShader(&shader, nullptr, nullptr);
    std::ostringstream out;
    const auto scope = rsf_ac7_capture_root_scope();
    out << "{\"kind\":\"dispatch\",\"qpc\":" << tick() << ",\"interval\":" << capture.interval
        << ",\"thread\":" << GetCurrentThreadId() << ",\"native_scope\":" << scope.id
        << ",\"native_view\":" << scope.view
        << ",\"groups\":[" << x << ',' << y << ',' << z << "],\"indirect\":" << uint64_t(uintptr_t(indirect))
        << ",\"offset\":" << offset << ",\"cs_hash\":" << shader_hash(shader.Get(), 5) << ",\"inputs\":[";
    ID3D11ShaderResourceView* views[128]{}; c->CSGetShaderResources(0, 128, views);
    bound_views(out, views);
    out << "],\"outputs\":[";
    ID3D11UnorderedAccessView* outputs[8]{}; c->CSGetUnorderedAccessViews(0, 8, outputs);
    bound_views(out, outputs);
    out << "],\"cs_cb\":";
    ID3D11Buffer* cb[14]{}; void* borrowed[14]{}; c->CSGetConstantBuffers(0, 14, cb);
    for (uint32_t i = 0; i < 14; ++i) borrowed[i] = cb[i];
    constants(capture, out, c, borrowed, shader_constants(shader.Get(), 5));
    for (auto* b : cb) if (b) b->Release();
    queued_scope(out);
    out << '}'; record_draw(capture, out.str());
}
catch (...) { capture_failed(capture); }

bool write_file(const char* path, const unsigned char* data, size_t bytes) noexcept
{
    FILE* f = std::fopen(path, "wb");
    if (!f) return false;
    const bool ok = bytes == 0 || std::fwrite(data, 1, bytes, f) == bytes;
    return std::fclose(f) == 0 && ok;
}

template<class List> MotionArtifactList byte_artifacts(const List& list) noexcept
{
    return {&list, list.size(), [](const void* user, size_t index) noexcept -> MotionArtifact {
        const auto& bytes = (*static_cast<const List*>(user))[index];
        return {reinterpret_cast<const unsigned char*>(bytes.data()), bytes.size()};
    }};
}

bool copy_checkpoint_roots(void* user, const RootRecord* records, size_t count) noexcept try
{
    auto& roots = *static_cast<std::vector<RootRecord>*>(user);
    roots.assign(records, records + count);
    return true;
}
catch (...) { return false; }
void checkpoint_native(MotionCaptureInterval capture) noexcept
{
    if (!rsf_ac7_capture_due_checkpoint(capture)) return;
    try {
        auto& s = state();
        std::string prefix;
        std::vector<RootRecord> roots;
        std::vector<std::vector<unsigned char>> blobs;
        bool copied;
        {
            std::lock_guard<std::mutex> lock(s.guard);
            if (!rsf_ac7_capture_current(capture.session)) return;
            prefix = s.prefix;
            copied = rsf_ac7_capture_copy_roots(&roots, copy_checkpoint_roots);
            if (copied) blobs = s.blobs;
        }
        if (!copied) { capture_failed(capture); return; }
        const auto files = byte_artifacts(blobs);
        const auto result = rsf_ac7_capture_publish_checkpoint(prefix.c_str(), write_file,
            roots.data(), roots.size(), &files);
        if (result == publication_failed) capture_failed(capture);
        else if (result == publication_partial) say("motion capture: native checkpoint could not be fully written");
    } catch (...) { capture_failed(capture); }
}

MotionArtifactStats finish_stats(const void* user) noexcept
{
    const auto& s = *static_cast<const Capture*>(user);
    LARGE_INTEGER frequency{}; QueryPerformanceFrequency(&frequency);
    return {frequency.QuadPart, s.installed.load(), s.roots_installed.load(), s.counters.draw_dropped,
        s.counters.blob_dropped, s.counters.readbacks, s.counters.readback_failed,
        s.counters.colour_readbacks, s.counters.colour_readback_failed, rsf_ac7_capture_failures()};
}
void finish() noexcept try
{
    auto& s = state();
    std::unique_lock<std::mutex> lock(s.guard);
    const MotionFinalArtifacts files{byte_artifacts(s.draws), byte_artifacts(s.blobs), &s, finish_stats};
    const auto result = rsf_ac7_capture_publish_finish(s.prefix.c_str(), write_file, &files);
    if (result == publication_failed) {
        lock.unlock();
        capture_failed();
        return;
    }
    say((std::string("motion capture finished: ") + s.prefix +
         (result == publication_complete ? "" : " (some writes failed)")).c_str());
}
catch (...) { capture_failed(); }

bool poll_capture(void* swapchain, bool* pressed) noexcept
{
    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(static_cast<IDXGISwapChain*>(swapchain)->GetDesc(&desc))) return false;
    DWORD process = 0; GetWindowThreadProcessId(GetForegroundWindow(), &process);
    *pressed = process == GetCurrentProcessId() && (GetAsyncKeyState(VK_F9) & 0x8000) != 0;
    return true;
}

bool prepare_capture(uint32_t session) noexcept try
{
    auto& s = state();
    SYSTEMTIME time{}; GetLocalTime(&time);
    char name[96]{};
    std::snprintf(name, sizeof(name), "\\motion-%04u%02u%02u-%02u%02u%02u-%lu-%u",
                  time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond,
                  GetCurrentProcessId(), session);
    std::string path = s.directory + name;
    if (!CreateDirectoryA(path.c_str(), nullptr)) {
        say("motion capture: output directory could not be created");
        return false;
    }
    std::lock_guard<std::mutex> lock(s.guard);
    s.prefix = std::move(path);
    s.draws.clear(); s.blobs.clear(); s.blob_bytes = 0;
    s.buffers.clear(); // Uploads are session-local.
    s.counters = {};
    s.geometry_interval = UINT32_MAX;
    s.geometry_reads.fill(0);
    for (auto& seen : s.geometry_seen) seen.clear();
    return true;
}
catch (...) { capture_failed(); return false; }

void capture_armed(MotionCaptureInterval capture) noexcept try
{
    say((std::string("motion capture armed: ") + state().prefix).c_str());
}
catch (...) { capture_failed(capture); }

void set_sample(uint32_t interval, char* prefix, uint32_t capacity) noexcept
{
    if (interval == UINT32_MAX) {
        rsf_frame_tap_set_research_callbacks(nullptr, nullptr, nullptr);
        return;
    }
    rsf_frame_tap_set_research_phase_callbacks(
        [](void*, const rsf_frame_tap_target_draw* d) { before_draw_capture(rsf_ac7_capture_sample(), d); },
        [](void*, const rsf_frame_tap_target_draw* d) { draw_capture(rsf_ac7_capture_sample(), d); },
        [](void*, void* context, uint32_t x, uint32_t y, uint32_t z, void* indirect, uint32_t offset) {
            compute_capture(rsf_ac7_capture_sample(), context, x, y, z, indirect, offset);
        }, nullptr);
    std::snprintf(prefix, capacity, "%s\\f%03u", state().prefix.c_str(), interval);
}

const MotionCaptureEffects capture_effects{poll_capture, prepare_capture, capture_armed, set_sample, finish};
} // namespace

extern "C" void rsf_ac7_motion_capture_configure(const char* directory, rsf_ac7_capture_log_fn log, void* user) try
{
    auto& s = state(); s.directory = directory ? directory : ""; s.log = log; s.user = user;
    const bool enabled = !s.directory.empty();
    rsf_ac7_capture_enable(enabled);
    if (enabled) say("motion capture configured: F9 records 60 intervals, sampled at 0/30/59");
}
catch (...) { capture_failed(); }
extern "C" int rsf_ac7_motion_capture_install(void)
{
    auto& s = state(); if (!rsf_ac7_capture_enabled() || s.installed) return s.installed ? 1 : 0;
    auto* base = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr));
    const MH_STATUS init = MH_Initialize();
    if (init != MH_OK && init != MH_ERROR_ALREADY_INITIALIZED) { say("motion capture: MinHook unavailable"); return 0; }
    install_roots(base);
    const unsigned char should[] = {0x40,0x53,0x48,0x83,0xec,0x20,0x4c,0x8b,0xda,0x48,0x8b,0xd9,0x45,0x84,0xc0};
    const unsigned char has[] = {0x40,0x53,0x48,0x83,0xec,0x60,0x80,0xb9,0x34,0x0c,0,0,0};
    const unsigned char history[] = {0x40,0x53,0x48,0x83,0xec,0x20,0x49,0x8b,0xd8,0x48,0x85,0xd2,0x74,0x3a};
    unsigned char actual[sizeof(should)]{};
    unsigned char size_gate[5]{};
    const bool size_matches = copy_memory(size_gate, base + 0x10fdd09, sizeof(size_gate)) &&
        (!std::memcmp(size_gate, "\x0f\x2f\xdd\x76\x38", 5) ||
         (native_owner.load() && !std::memcmp(size_gate, "\x0f\x2f\xdd\x90\x90", 5)));
    if (!base || !copy_memory(actual, base + 0x10fdc00, sizeof(should)) || std::memcmp(actual, should, sizeof(should)) ||
        !copy_memory(actual, base + 0x1183820, sizeof(has)) || std::memcmp(actual, has, sizeof(has)) ||
        !copy_memory(actual, base + 0x10ee670, sizeof(history)) || std::memcmp(actual, history, sizeof(history)) ||
        !size_matches) {
        say("motion capture engine hooks refused: expected AC7 bytes differ"); return 0;
    }
    void* detours[] = {reinterpret_cast<void*>(&hooked_should), reinterpret_cast<void*>(&hooked_has), reinterpret_cast<void*>(&hooked_history)};
    void** originals[] = {reinterpret_cast<void**>(&original_should), reinterpret_cast<void**>(&original_has), reinterpret_cast<void**>(&original_history)};
    const uint32_t rvas[] = {0x10fdc00, 0x1183820, 0x10ee670};
    for (uint32_t i = 0; i < 3; ++i) hook_targets[i] = base + rvas[i];
    if (rsf::ac7::install_hooks(hook_targets, detours, originals, 3)) {
        s.installed = true; say("motion capture engine hooks installed; original decisions preserved"); return 1;
    }
    std::fill(std::begin(hook_targets), std::end(hook_targets), nullptr);
    say("motion capture engine hooks failed and were rolled back"); return 0;
}
extern "C" void rsf_ac7_motion_capture_shutdown(void)
{
    auto& s = state();
    rsf_ac7_capture_shutdown(&capture_effects);
    if (s.installed) rsf::ac7::remove_hooks(hook_targets, 3);
    if (s.roots_installed) rsf::ac7::remove_hooks(root_targets, 6);
    s.roots_installed = false;
    s.installed = false;
}
extern "C" void rsf_ac7_motion_capture_shader(void* shader, uint32_t stage, const void* bytes, uint32_t size)
{
    if (!rsf_ac7_capture_observe_shader(shader, stage, bytes, size, disassemble_constants)) capture_failed();
}
extern "C" void rsf_ac7_motion_capture_buffer(void*, void* buffer, const void* initial, uint32_t bytes, uint32_t bind_flags) try
{
    auto& s = state(); if (!rsf_ac7_capture_enabled()) return;
    { std::lock_guard<std::mutex> lock(s.guard); s.buffers.erase(buffer); }
    if (initial && (bind_flags & D3D11_BIND_CONSTANT_BUFFER)) rsf_ac7_motion_capture_upload(buffer, initial, bytes);
}
catch (...) { capture_failed(); }
extern "C" void rsf_ac7_motion_capture_upload(void* buffer, const void* bytes, uint32_t size)
{
    // Snapshots only matter inside a capture; copying every dynamic upload otherwise costs a lock and a vector.
    const auto capture = rsf_ac7_capture_interval();
    if (!rsf_ac7_capture_enabled() || !capture || !buffer || !bytes || !size || size > 8192) return;
    try {
        auto& s = state(); std::lock_guard<std::mutex> lock(s.guard);
        if (!rsf_ac7_capture_current(capture.session)) return;
        if (!s.buffers.contains(buffer) && s.buffers.size() >= 2048) return;
        auto& snapshot = s.buffers[buffer]; snapshot.serial = ++s.upload_serial;
        snapshot.data.assign(static_cast<const unsigned char*>(bytes), static_cast<const unsigned char*>(bytes) + size);
    } catch (...) { capture_failed(capture); }
}
extern "C" void rsf_ac7_motion_capture_layout(void* layout, const rsf_observer_layout_element* elements,
    uint32_t copied, uint32_t count)
{
    if (!rsf_ac7_capture_observe_layout(layout, elements, copied, count)) capture_failed();
}
namespace {
void capture_geometry(MotionCaptureInterval capture, const rsf_frame_tap_geometry* draw) try
{
    auto& s = state();
    if (!draw || !capture) return;
    const bool native_velocity=draw->format==DXGI_FORMAT_R16G16_UNORM && draw->depth_view;
    if (!native_velocity) {
        const auto role = rsf_ac7_capture_geometry_scope().role;
        if (role != RSF_GAME_RENDER_TRANSLUCENCY && role != RSF_GAME_RENDER_TRANSLUCENT_MESH) return;
    }
    const auto interval = capture.interval;
    // Bounded samples carry actual OldPosition/particle IDs and mesh vertex histories. Never
    // retain a borrowed buffer expecting a later frame to still contain the same vertices.
    MotionLayoutSnapshot layout{};
    {
        std::lock_guard<std::mutex> lock(s.guard);
        if (rsf_ac7_capture_interval() != capture) return;
        if (s.geometry_interval != interval) {
            s.geometry_interval = interval;
            s.geometry_reads.fill(0);
            for (auto& seen : s.geometry_seen) seen.clear();
        }
        if (!rsf_ac7_capture_find_layout(draw->input_layout, &layout)) return;
    }
    uint32_t slots=0;bool fields[16]{};bool old3=false;
    for (uint32_t i = 0; i < layout.copied; ++i) {
        const auto& e = layout.elements[i];
        if (e.input_slot<32) slots|=uint32_t(1)<<e.input_slot;
        if (e.semantic_index<16 && e.format==DXGI_FORMAT_R32G32B32A32_FLOAT) fields[e.semantic_index]=true;
        if (e.semantic_index==1 && e.format==DXGI_FORMAT_R32G32B32_FLOAT) old3=true;
    }
    const uint32_t bucket=native_velocity ? 4u : fields[8] && fields[9] && fields[10] ? 2u :
        fields[0] && old3 && fields[2] && fields[3] && fields[4] ? 3u : fields[0] && fields[1] ? 1u : 0u;
    constexpr uint32_t limits[]{8,12,12,8,12};
    const auto hash = shader_hash(draw->vertex_shader, 0);
    {
        std::lock_guard<std::mutex> lock(s.guard);
        if (rsf_ac7_capture_interval() != capture || s.geometry_reads[bucket] >= limits[bucket]) return;
        // Reserve independent budgets for sprites, mesh particles, curves and native velocity.
        // Repeated particle sections must not hide later producers. These are capture candidates.
        if (bucket >= 1 && bucket <= 3 && !s.geometry_seen[bucket].emplace(hash, draw->input_layout).second) return;
    }
    auto* context = static_cast<ID3D11DeviceContext*>(draw->context);
    if (!context || context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return;
    ComPtr<ID3D11Device> device; context->GetDevice(&device);
    std::ostringstream out;
    const auto execution_pass = rsf_ac7_capture_geometry_scope();
    out << "{\"kind\":\"geometry\",\"interval\":" << interval
        << ",\"qpc\":" << tick() << ",\"layout\":" << uint64_t(uintptr_t(draw->input_layout))
        << ",\"vs_hash\":" << hash << ",\"geometry_bucket\":" << bucket
        << ",\"native_role\":" << execution_pass.role << ",\"native_frame\":" << execution_pass.native_frame
        << ",\"primitive\":" << execution_pass.primitive << ",\"material\":" << execution_pass.material
        << ",\"geometry_capacity\":" << execution_pass.geometry_capacity
        << ",\"draw_kind\":" << draw->kind << ",\"count\":" << draw->count
        << ",\"start\":" << draw->start << ",\"base_vertex\":" << draw->base_vertex
        << ",\"instances\":" << draw->instances << ",\"start_instance\":" << draw->start_instance
        << ",\"layout_count\":" << layout.count << ",\"elements\":[";
    for (uint32_t i = 0; i < layout.copied; ++i) {
        const auto& e = layout.elements[i];
        if (i) out << ',';
        out << "{\"attribute\":" << e.semantic_index << ",\"format\":" << e.format
            << ",\"slot\":" << e.input_slot << ",\"offset\":" << e.byte_offset
            << ",\"instance\":" << e.per_instance << '}';
    }
    out << "],\"streams\":["; bool first = true;
    const auto can_read = [&] {
        std::lock_guard<std::mutex> lock(s.guard);
        return rsf_ac7_capture_interval() == capture && s.geometry_reads[bucket] < limits[bucket];
    };
    auto read_buffer=[&](ID3D11Buffer* buffer,uint32_t offset,uint32_t maximum=16384u) {
        std::vector<unsigned char> data;
        if (!buffer) return data;
        D3D11_BUFFER_DESC desc{};buffer->GetDesc(&desc);
        if (offset>=desc.ByteWidth) return data;
        const uint32_t bytes=std::min(desc.ByteWidth-offset,std::min(maximum,16384u));
        if (!bytes) return data;
        D3D11_BUFFER_DESC staging{};staging.ByteWidth=bytes;
        staging.Usage=D3D11_USAGE_STAGING;staging.CPUAccessFlags=D3D11_CPU_ACCESS_READ;
        {
            std::lock_guard<std::mutex> lock(s.guard);
            if (rsf_ac7_capture_interval() != capture || s.geometry_reads[bucket] >= limits[bucket]) return data;
            ++s.geometry_reads[bucket];
        }
        ComPtr<ID3D11Buffer> readable;
        if (FAILED(device->CreateBuffer(&staging,nullptr,&readable))) return data;
        const D3D11_BOX box{offset,0,0,offset+bytes,1,1};
        context->CopySubresourceRegion(readable.Get(),0,0,0,0,buffer,0,&box);
        D3D11_MAPPED_SUBRESOURCE mapped{};
        if (SUCCEEDED(context->Map(readable.Get(),0,D3D11_MAP_READ,0,&mapped))) {
            data.resize(bytes);std::memcpy(data.data(),mapped.pData,bytes);context->Unmap(readable.Get(),0);
        }
        return data;
    };
    struct Predication {
        ID3D11DeviceContext* context;ComPtr<ID3D11Predicate> predicate;BOOL value=FALSE;
        explicit Predication(ID3D11DeviceContext* c):context(c) {context->GetPredication(&predicate,&value);context->SetPredication(nullptr,FALSE);}
        ~Predication() {context->SetPredication(predicate.Get(),value);}
    } predication(context);
    for (uint32_t slot = 0; slot < 32; ++slot) {
        if (!(slots&(uint32_t(1)<<slot))) continue;
        if (!can_read()) break;
        auto* buffer = static_cast<ID3D11Buffer*>(draw->vertex_buffers[slot]);
        if (!buffer) continue;
        D3D11_BUFFER_DESC desc{}; buffer->GetDesc(&desc);
        if (!desc.ByteWidth || draw->offsets[slot] >= desc.ByteWidth) continue;
        const uint32_t bytes = std::min(desc.ByteWidth - draw->offsets[slot], 16384u);
        auto data=read_buffer(buffer,draw->offsets[slot]);
        const auto blob = data.empty() ? 0u : save_blob(capture, std::move(data));
        if (!first) out << ','; first = false;
        out << "{\"slot\":" << slot << ",\"buffer\":" << uint64_t(uintptr_t(buffer))
            << ",\"stride\":" << draw->strides[slot] << ",\"offset\":" << draw->offsets[slot]
            << ",\"source_bytes\":" << desc.ByteWidth << ",\"sample_bytes\":" << bytes
            << ",\"blob\":" << blob << '}';
    }
    out << "],\"vs_buffers\":[";first=true;
    for (const uint32_t slot:std::array<uint32_t,3>{0,1,127}) {
        if ((bucket!=4 && !(bucket==2 && slot==127)) || !can_read()) continue;
        ComPtr<ID3D11ShaderResourceView> view;context->VSGetShaderResources(slot,1,&view);
        if (!view) continue;
        ComPtr<ID3D11Resource> resource;view->GetResource(&resource);ComPtr<ID3D11Buffer> buffer;
        if (FAILED(resource.As(&buffer))) continue;
        D3D11_SHADER_RESOURCE_VIEW_DESC desc{};view->GetDesc(&desc);D3D11_BUFFER_DESC b{};buffer->GetDesc(&b);
        uint32_t stride=b.StructureByteStride;
        if (desc.Format==DXGI_FORMAT_R32G32B32A32_FLOAT) stride=16;
        else if (desc.Format==DXGI_FORMAT_R32G32B32_FLOAT) stride=12;
        else if (desc.Format==DXGI_FORMAT_R32G32_FLOAT) stride=8;
        else if (desc.Format==DXGI_FORMAT_R32_FLOAT || desc.Format==DXGI_FORMAT_R32_TYPELESS) stride=4;
        const uint32_t element=desc.ViewDimension==D3D11_SRV_DIMENSION_BUFFEREX ? desc.BufferEx.FirstElement : desc.Buffer.FirstElement;
        const uint32_t elements=desc.ViewDimension==D3D11_SRV_DIMENSION_BUFFEREX ? desc.BufferEx.NumElements : desc.Buffer.NumElements;
        if (!stride || element>b.ByteWidth/stride || elements>UINT32_MAX/stride) continue;
        const uint32_t offset=element*stride;auto data=read_buffer(buffer.Get(),offset,elements*stride);
        const auto bytes=data.size();
        const auto blob=data.empty() ? 0u : save_blob(capture, std::move(data));
        if (!first) out << ',';first=false;
        out << "{\"slot\":" << slot << ",\"resource\":" << uint64_t(uintptr_t(buffer.Get()))
            << ",\"format\":" << desc.Format << ",\"offset\":" << offset << ",\"source_bytes\":" << b.ByteWidth
            << ",\"sample_bytes\":" << bytes << ",\"blob\":" << blob << '}';
    }
    out << ']'; queued_scope(out); out << '}'; record_draw(capture, out.str());
}
catch (...) { capture_failed(capture); }
} // namespace

extern "C" void rsf_ac7_motion_capture_geometry(const rsf_frame_tap_geometry* draw)
{ capture_geometry(rsf_ac7_capture_sample(), draw); }
extern "C" void rsf_ac7_motion_capture_native_owner(uint32_t enabled)
{ native_owner.store(enabled != 0); }
extern "C" void rsf_ac7_motion_capture_native_pass(const rsf_game_render_pass* pass, uint32_t begin)
{
    if (!rsf_ac7_capture_execution_pass(pass)) return;
    const auto capture = rsf_ac7_capture_sample();
    if (!capture) return;
    auto r = root_record(capture, begin ? rhi_scope_begin : rhi_scope_restore, nullptr);
    rsf_ac7_capture_execution_root(&r);
    record_root(r);
}
extern "C" void rsf_ac7_motion_capture_request(void)
{ rsf_ac7_capture_request(); }
extern "C" int rsf_ac7_motion_capture_present(void* swapchain, char* prefix, uint32_t capacity)
{
    return swapchain && prefix && capacity && rsf_ac7_capture_present(swapchain, prefix, capacity, &capture_effects);
}
