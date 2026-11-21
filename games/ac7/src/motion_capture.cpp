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
#include <atomic>
#include <cstdio>
#include <cstring>
#include <iterator>
#include <mutex>
#include <new>
#include <string>
#include <utility>
#include <vector>

using Microsoft::WRL::ComPtr;

namespace {
constexpr uint32_t max_blobs = 4096;
constexpr size_t blob_budget = 32u * 1024u * 1024u;

struct Capture {
    std::mutex guard;
    std::atomic<bool> installed{false};
    std::atomic<bool> roots_installed{false};
    std::string directory, prefix;
    rsf_ac7_capture_log_fn log = nullptr;
    void* user = nullptr;
    std::vector<std::vector<unsigned char>> blobs;
    size_t blob_bytes = 0;
    uint32_t blob_dropped = 0;
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
        ++s.blob_dropped; return 0;
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
MotionQueuedDraw queued_draw()
{
    MotionQueuedDraw draw{};
    if (state().roots_installed && copy_memory(&draw.command, current_rhi_command_address, sizeof(draw.command)) &&
        draw.command && read(draw.command, 8, draw.execute)) {
        auto& s = state(); std::lock_guard<std::mutex> lock(s.guard);
        draw.scope = rsf_ac7_capture_queued_scope(draw.command, draw.execute, true);
    } else draw.scope = rsf_ac7_capture_queued_scope(draw.command, draw.execute, false);
    return draw;
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
bool copy_upload(void* user, const unsigned char* bytes, size_t size) noexcept try
{
    static_cast<std::vector<unsigned char>*>(user)->assign(bytes, bytes + size);
    return true;
}
catch (...) { return false; }

template<class Visit>
void visit_constants(MotionCaptureInterval capture, ID3D11DeviceContext* context, void* const* buffers,
                     uint32_t mask, bool lighting, Visit visit)
{
    for (uint32_t slot = 0; slot < 14; ++slot) {
        if (!buffers[slot] || !(mask & (1u << slot))) continue;
        std::vector<unsigned char> bytes;
        MotionConstantRecord record{};
        const auto source = rsf_ac7_capture_constant_source(capture, buffers[slot], lighting, &record, &bytes, copy_upload);
        if (source == constant_stopped) break;
        if (source == constant_failed) throw std::bad_alloc{};
        bool fallback = false;
        if (source == constant_try_read) {
            auto* buffer = static_cast<ID3D11Buffer*>(buffers[slot]);
            D3D11_BUFFER_DESC desc{}; buffer->GetDesc(&desc);
            const auto read = rsf_ac7_capture_reserve_constant_read(capture, desc.ByteWidth, lighting);
            if (read == constant_read_stopped) break;
            fallback = read == constant_read_reserved;
            if (fallback) {
                bytes.resize(desc.ByteWidth);
                ComPtr<ID3D11Device> device; context->GetDevice(&device);
                if (rsf_read_constant_buffer(device.Get(), context, buffer, bytes.data(), desc.ByteWidth) != RSF_CONSTANT_BUFFER_OK) {
                    bytes.clear();
                    rsf_ac7_capture_constant_read_failed(capture);
                }
            }
        }
        const uint32_t id = bytes.empty() ? 0 : save_blob(capture, std::move(bytes));
        visit(MotionBoundConstant{record.buffer, record.upload_serial, slot, id, uint32_t(fallback)});
    }
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
// These dimension values select the optional fields in the Rust row writers.
static_assert(D3D11_RESOURCE_DIMENSION_TEXTURE2D == 3 && D3D11_RESOURCE_DIMENSION_TEXTURE3D == 4);
MotionResource resource_facts(ID3D11Resource* r)
{
    D3D11_RESOURCE_DIMENSION dimension{}; r->GetType(&dimension);
    MotionResource facts{uint64_t(uintptr_t(r)), uint32_t(dimension), 0, 0, 0, 0};
    if (dimension == D3D11_RESOURCE_DIMENSION_TEXTURE2D) {
        D3D11_TEXTURE2D_DESC d{}; static_cast<ID3D11Texture2D*>(r)->GetDesc(&d);
        facts.width = d.Width; facts.height = d.Height; facts.format = uint32_t(d.Format);
    } else if (dimension == D3D11_RESOURCE_DIMENSION_TEXTURE3D) {
        D3D11_TEXTURE3D_DESC d{}; static_cast<ID3D11Texture3D*>(r)->GetDesc(&d);
        facts.width = d.Width; facts.height = d.Height; facts.depth = d.Depth; facts.format = uint32_t(d.Format);
    }
    return facts;
}

template<class View, size_t N, class Visit> void visit_bound_views(View* (&views)[N], Visit visit)
{
    for (size_t i = 0; i < N; ++i) {
        if (!views[i]) continue;
        ComPtr<ID3D11Resource> r; views[i]->GetResource(&r);
        visit(MotionBoundView{resource_facts(r.Get()), uint32_t(i)});
        views[i]->Release();
    }
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
    default: return rsf_ac7_capture_is_cloud_composite(hash) ? "cloud_composite_after" : nullptr;
    }
}
MotionColourSnapshot colour_snapshot(MotionCaptureInterval capture, ID3D11DeviceContext* context,
                                     ID3D11Resource* resource, const char* role, bool lighting = false)
{
    if (!role || !resource || !rsf_ac7_capture_colour_snapshot_available(capture, lighting)) return {};
    ComPtr<ID3D11Texture2D> texture;
    if (FAILED(resource->QueryInterface(IID_PPV_ARGS(&texture)))) return {};
    auto& s = state();
    MotionColourSnapshot snapshot{};
    std::string path;
    {
        // Keep the accepted ordinals and their paths tied to this native session's directory.
        std::lock_guard<std::mutex> lock(s.guard);
        const auto ordinal = rsf_ac7_capture_reserve_colour_snapshot(capture, lighting);
        if (ordinal == UINT32_MAX) return {};
        std::snprintf(snapshot.name, sizeof(snapshot.name), "f%03u_stage_%s_%03u", capture.interval, role, ordinal);
        path = s.prefix + "\\" + snapshot.name;
    }
    ComPtr<ID3D11Device> device; context->GetDevice(&device);
    rsf_texture_dump_options options{}; options.struct_size = sizeof(options);
    options.abi_version = RSF_TEXTURE_DUMP_ABI_VERSION; options.output_prefix_utf8 = path.c_str();
    snapshot.result = rsf_dump_texture_bytes(device.Get(), context, texture.Get(), &options);
    if (snapshot.result != RSF_TEXTURE_OK) rsf_ac7_capture_colour_snapshot_failed(capture);
    return snapshot;
}
MotionDrawTarget sampled_target(MotionCaptureInterval capture, ID3D11DeviceContext* context,
    ID3D11Resource* resource, const char* role, bool lighting, uint32_t slot)
{
    const auto facts = resource_facts(resource);
    const auto snapshot = colour_snapshot(capture, context, resource, role, lighting);
    return {{facts, slot}, snapshot};
}
void before_draw_capture(MotionCaptureInterval capture, const rsf_frame_tap_target_draw* d) try
{
    if (!capture) return;
    MotionBeforeDrawPlan plan{};
    if (!rsf_ac7_capture_plan_before_draw(capture, d->target_width, d->target_height, d->pixel_shader, &plan)) return;
    auto* context = static_cast<ID3D11DeviceContext*>(d->context);
    const bool lighting = plan.lighting != 0;
    MotionBeforeDraw row{};
    row.qpc = tick(); row.thread = GetCurrentThreadId();
    if (!lighting) row.draw_index = d->draw_index;
    row.colour = sampled_target(capture, context, static_cast<ID3D11Resource*>(d->render_target),
        plan.colour_role, lighting, 0);

    ComPtr<ID3D11RenderTargetView> colour_target, loss_target;
    if (plan.loss_role) {
        ID3D11RenderTargetView* targets[2]{}; context->OMGetRenderTargets(2, targets, nullptr);
        colour_target.Attach(targets[0]); loss_target.Attach(targets[1]);
        if (loss_target) {
            ComPtr<ID3D11Resource> texture; loss_target->GetResource(&texture);
            row.loss = sampled_target(capture, context, texture.Get(), plan.loss_role, lighting, 1);
        }
    }

    ID3D11ShaderResourceView* views[6]{}; context->PSGetShaderResources(0, plan.input_slots, views);
    ComPtr<ID3D11ShaderResourceView> retained[6];
    for (uint32_t slot = 0; slot < plan.input_slots; ++slot) retained[slot].Attach(views[slot]);
    for (uint32_t slot = 0; slot < plan.input_slots; ++slot) {
        if (!retained[slot]) continue;
        ComPtr<ID3D11Resource> texture; retained[slot]->GetResource(&texture);
        row.inputs[row.input_count++] = sampled_target(capture, context, texture.Get(),
            plan.input_roles[slot], lighting, slot);
    }

    ComPtr<ID3D11Buffer> retained_buffers[2];
    if (lighting) {
        ID3D11SamplerState* samplers[6]{}; context->PSGetSamplers(0, plan.sampler_slots, samplers);
        for (uint32_t slot = 0; slot < plan.sampler_slots; ++slot) if (samplers[slot]) {
            D3D11_SAMPLER_DESC sampler{}; samplers[slot]->GetDesc(&sampler); samplers[slot]->Release();
            row.samplers[row.sampler_count++] = {slot, uint32_t(sampler.Filter),
                {uint32_t(sampler.AddressU), uint32_t(sampler.AddressV), uint32_t(sampler.AddressW)},
                uint32_t(sampler.ComparisonFunc), sampler.MinLOD, sampler.MaxLOD};
        }
        // Read the two live shader inputs before the light executes, using the reserved quota.
        ID3D11Buffer* raw[2]{}; context->PSGetConstantBuffers(0, plan.constant_slots, raw);
        void* buffers[14]{};
        for (uint32_t i = 0; i < plan.constant_slots; ++i) { retained_buffers[i].Attach(raw[i]); buffers[i] = raw[i]; }
        const uint32_t mask = (1u << plan.constant_slots) - 1;
        visit_constants(capture, context, buffers, mask, lighting,
            [&](const MotionBoundConstant& record) { row.constants[row.constant_count++] = record; });
        row.queued = queued_draw();
    }
    if (!rsf_ac7_capture_record_before_draw(capture, &plan, &row)) capture_failed(capture);
}
catch (...) { capture_failed(capture); }
void draw_capture(MotionCaptureInterval capture, const rsf_frame_tap_target_draw* d) try
{
    if (!capture || !rsf_ac7_capture_can_record_draw(capture)) return;
    auto* c = static_cast<ID3D11DeviceContext*>(d->context);
    const uint32_t ps = shader_hash(d->pixel_shader, 1);
    const bool light_after = rsf_ac7_capture_take_lighting_after(capture, ps);
    ComPtr<ID3D11GeometryShader> gs; ComPtr<ID3D11HullShader> hs; ComPtr<ID3D11DomainShader> ds;
    c->GSGetShader(&gs, nullptr, nullptr); c->HSGetShader(&hs, nullptr, nullptr); c->DSGetShader(&ds, nullptr, nullptr);
    MotionDraw draw{};
    draw.root = rsf_ac7_capture_root_scope();
    draw.qpc = tick();
    draw.thread = GetCurrentThreadId();
    draw.draw_index = d->draw_index;
    draw.target = uint64_t(uintptr_t(d->render_target));
    draw.format = d->target_format;
    draw.width = d->target_width;
    draw.height = d->target_height;
    draw.target_count = d->target_count;
    draw.depth_bound = d->depth_bound;
    draw.indexed = d->indexed;
    draw.elements = d->element_count;
    draw.shader_hashes[0] = shader_hash(d->vertex_shader, 0);
    draw.shader_hashes[1] = ps;
    draw.shader_hashes[2] = shader_hash(gs.Get(), 2);
    draw.shader_hashes[3] = shader_hash(hs.Get(), 3);
    draw.shader_hashes[4] = shader_hash(ds.Get(), 4);
    draw.layout = uint64_t(uintptr_t(d->input_layout));
    draw.topology = d->topology;
    draw.stride = d->vertex_stride;
    draw.viewport_x = d->viewport_x;
    draw.viewport_y = d->viewport_y;
    draw.viewport_width = d->viewport_width;
    draw.viewport_height = d->viewport_height;
    static_assert(std::size(draw.inputs) == RSF_FRAME_TAP_MAX_INPUTS);
    if (d->input_count > std::size(draw.inputs)) { capture_failed(capture); return; }
    draw.input_count = d->input_count;
    for (uint32_t i = 0; i < draw.input_count; ++i) {
        const auto& input = d->inputs[i];
        draw.inputs[i] = {uint64_t(uintptr_t(input.texture)), input.slot, input.width, input.height, input.format};
    }
    ID3D11ShaderResourceView* views[128]{}; c->PSGetShaderResources(0, 128, views);
    visit_bound_views(views, [&](const MotionBoundView& view) { draw.ps_resources[draw.ps_resource_count++] = view; });
    c->VSGetShaderResources(0, 128, views);
    visit_bound_views(views, [&](const MotionBoundView& view) { draw.vs_resources[draw.vs_resource_count++] = view; });
    ID3D11RenderTargetView* targets[8]{}; ComPtr<ID3D11DepthStencilView> depth;
    c->OMGetRenderTargets(8, targets, &depth);
    ComPtr<ID3D11RenderTargetView> retained[8];
    for (uint32_t i = 0; i < 8; ++i) retained[i].Attach(targets[i]);
    const bool large = d->target_width >= 400 && d->target_height >= 200;
    for (uint32_t i = 0; i < 8; ++i) {
        auto target = std::move(retained[i]);
        if (!target) continue;
        ComPtr<ID3D11Resource> resource; target->GetResource(&resource);
        auto& record = draw.render_targets[draw.render_target_count++];
        record.view = {resource_facts(resource.Get()), i};
        if (i == 0) {
            // The contact-light shaders are separate from the colour_stage producers.
            const auto* role = large ? colour_stage(ps) : nullptr;
            if (light_after) role = "contact_light_after";
            record.snapshot = colour_snapshot(capture, c, resource.Get(), role, light_after);
        } else if (i == 1 && large && rsf_ac7_capture_is_cloud_composite(ps)) {
            record.snapshot = colour_snapshot(capture, c, resource.Get(), "cloud_loss_after");
        }
    }
    if (depth) {
        ComPtr<ID3D11Resource> resource; depth->GetResource(&resource);
        draw.depth = resource_facts(resource.Get());
    }
    void* const shaders[]{d->vertex_shader, d->pixel_shader, gs.Get(), hs.Get(), ds.Get()};
    void* const* const buffers[]{d->vertex_constants, d->pixel_constants, d->geometry_constants,
        d->hull_constants, d->domain_constants};
    for (uint32_t i = 0; i < 5; ++i) {
        if (!shaders[i]) continue;
        auto& constants = draw.constants[i];
        visit_constants(capture, c, buffers[i], shader_constants(shaders[i], i), false,
            [&](const MotionBoundConstant& record) { constants.entries[constants.count++] = record; });
    }
    draw.queued = queued_draw();
    if (!rsf_ac7_capture_record_draw(capture, &draw)) capture_failed(capture);
}
catch (...) { capture_failed(capture); }
void compute_capture(MotionCaptureInterval capture, void* context, uint32_t x, uint32_t y, uint32_t z, void* indirect, uint32_t offset) try
{
    if (!capture || !rsf_ac7_capture_can_record_draw(capture)) return;
    auto* c = static_cast<ID3D11DeviceContext*>(context);
    ComPtr<ID3D11ComputeShader> shader; c->CSGetShader(&shader, nullptr, nullptr);
    MotionDispatch dispatch{};
    dispatch.root = rsf_ac7_capture_root_scope();
    dispatch.qpc = tick();
    dispatch.thread = GetCurrentThreadId();
    dispatch.groups[0] = x; dispatch.groups[1] = y; dispatch.groups[2] = z;
    dispatch.indirect = uint64_t(uintptr_t(indirect));
    dispatch.offset = offset;
    dispatch.cs_hash = shader_hash(shader.Get(), 5);
    ID3D11ShaderResourceView* views[128]{}; c->CSGetShaderResources(0, 128, views);
    visit_bound_views(views, [&](const MotionBoundView& view) { dispatch.inputs[dispatch.input_count++] = view; });
    ID3D11UnorderedAccessView* outputs[8]{}; c->CSGetUnorderedAccessViews(0, 8, outputs);
    visit_bound_views(outputs, [&](const MotionBoundView& view) { dispatch.outputs[dispatch.output_count++] = view; });
    ID3D11Buffer* cb[14]{}; void* borrowed[14]{}; c->CSGetConstantBuffers(0, 14, cb);
    for (uint32_t i = 0; i < 14; ++i) borrowed[i] = cb[i];
    visit_constants(capture, c, borrowed, shader_constants(shader.Get(), 5), false,
        [&](const MotionBoundConstant& record) { dispatch.constants[dispatch.constant_count++] = record; });
    for (auto* b : cb) if (b) b->Release();
    dispatch.queued = queued_draw();
    if (!rsf_ac7_capture_record_dispatch(capture, &dispatch)) capture_failed(capture);
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
    return {frequency.QuadPart, s.installed.load(), s.roots_installed.load(), s.blob_dropped, rsf_ac7_capture_failures()};
}
void finish() noexcept try
{
    auto& s = state();
    std::unique_lock<std::mutex> lock(s.guard);
    const MotionFinalArtifacts files{byte_artifacts(s.blobs), &s, finish_stats};
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
    s.blobs.clear(); s.blob_bytes = 0;
    s.blob_dropped = 0;
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
extern "C" void rsf_ac7_motion_capture_buffer(void*, void* buffer, const void* initial, uint32_t bytes, uint32_t bind_flags)
{
    const auto capture = rsf_ac7_capture_interval();
    const auto* data = (bind_flags & D3D11_BIND_CONSTANT_BUFFER) ? static_cast<const unsigned char*>(initial) : nullptr;
    if (!rsf_ac7_capture_store_upload(capture, buffer, data, bytes, true)) capture_failed(capture);
}
extern "C" void rsf_ac7_motion_capture_upload(void* buffer, const void* bytes, uint32_t size)
{
    const auto capture = rsf_ac7_capture_interval();
    if (!rsf_ac7_capture_store_upload(capture, buffer, static_cast<const unsigned char*>(bytes), size, false))
        capture_failed(capture);
}
extern "C" void rsf_ac7_motion_capture_layout(void* layout, const rsf_observer_layout_element* elements,
    uint32_t copied, uint32_t count)
{
    if (!rsf_ac7_capture_observe_layout(layout, elements, copied, count)) capture_failed();
}
namespace {
// The Rust capture classifies the target and the copied input layout with these DXGI values.
static_assert(DXGI_FORMAT_R32G32B32A32_FLOAT == 2 && DXGI_FORMAT_R32G32B32_FLOAT == 6 &&
    DXGI_FORMAT_R16G16_UNORM == 35);

void capture_geometry(MotionCaptureInterval capture, const rsf_frame_tap_geometry* draw) try
{
    if (!draw || !capture) return;
    MotionGeometryPlan plan{};
    const auto admission = rsf_ac7_capture_plan_geometry(capture, draw->format, draw->depth_view != nullptr,
        draw->input_layout, draw->vertex_shader, &plan);
    if (admission == geometry_failed) { capture_failed(capture); return; }
    if (admission != geometry_admitted) return;

    auto* context = static_cast<ID3D11DeviceContext*>(draw->context);
    if (!context || context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return;
    ComPtr<ID3D11Device> device; context->GetDevice(&device);
    MotionGeometry row{};
    row.scope = rsf_ac7_capture_geometry_scope();
    row.qpc = tick();
    row.layout = uint64_t(uintptr_t(draw->input_layout));
    row.kind = draw->kind;
    row.count = draw->count;
    row.start = draw->start;
    row.base_vertex = draw->base_vertex;
    row.instances = draw->instances;
    row.start_instance = draw->start_instance;

    const auto can_read = [&] { return rsf_ac7_capture_geometry_read(capture, plan.bucket, false); };
    constexpr uint32_t max_sample_bytes = 16384u;
    // Copy while the callback still owns the binding and its contents. Later uploads can reuse it.
    const auto read_buffer = [&](ID3D11Buffer* buffer, uint32_t offset, uint32_t maximum) {
        std::vector<unsigned char> data;
        D3D11_BUFFER_DESC desc{}; buffer->GetDesc(&desc);
        if (offset >= desc.ByteWidth) return data;
        const uint32_t bytes = std::min(desc.ByteWidth - offset, std::min(maximum, max_sample_bytes));
        if (!bytes) return data;
        D3D11_BUFFER_DESC staging{};
        staging.ByteWidth = bytes;
        staging.Usage = D3D11_USAGE_STAGING;
        staging.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        if (!rsf_ac7_capture_geometry_read(capture, plan.bucket, true)) return data;
        ComPtr<ID3D11Buffer> readable;
        if (FAILED(device->CreateBuffer(&staging, nullptr, &readable))) return data;
        const D3D11_BOX box{offset, 0, 0, offset + bytes, 1, 1};
        context->CopySubresourceRegion(readable.Get(), 0, 0, 0, 0, buffer, 0, &box);
        D3D11_MAPPED_SUBRESOURCE mapped{};
        if (SUCCEEDED(context->Map(readable.Get(), 0, D3D11_MAP_READ, 0, &mapped))) {
            data.resize(bytes);
            std::memcpy(data.data(), mapped.pData, bytes);
            context->Unmap(readable.Get(), 0);
        }
        return data;
    };
    struct Predication {
        ID3D11DeviceContext* context;
        ComPtr<ID3D11Predicate> predicate;
        BOOL value = FALSE;
        explicit Predication(ID3D11DeviceContext* c) : context(c) {
            context->GetPredication(&predicate, &value);
            context->SetPredication(nullptr, FALSE);
        }
        ~Predication() { context->SetPredication(predicate.Get(), value); }
    } predication(context);

    static_assert(std::size(row.streams) == sizeof(draw->vertex_buffers) / sizeof(draw->vertex_buffers[0]));
    for (uint32_t slot = 0; slot < std::size(row.streams); ++slot) {
        if (!(plan.ia_slots & (uint32_t(1) << slot))) continue;
        if (!can_read()) break;
        auto* buffer = static_cast<ID3D11Buffer*>(draw->vertex_buffers[slot]);
        if (!buffer) continue;
        D3D11_BUFFER_DESC desc{}; buffer->GetDesc(&desc);
        const uint32_t offset = draw->offsets[slot];
        if (offset >= desc.ByteWidth) continue;
        const uint32_t bytes = std::min(desc.ByteWidth - offset, max_sample_bytes);
        auto data = read_buffer(buffer, offset, max_sample_bytes);
        const auto blob = data.empty() ? 0u : save_blob(capture, std::move(data));
        row.streams[row.stream_count++] = {
            uint64_t(uintptr_t(buffer)), slot, draw->strides[slot], offset, desc.ByteWidth, bytes, blob,
        };
    }
    static_assert(std::size(row.vs_buffers) == std::size(plan.vs_slots));
    for (uint32_t index = 0; index < plan.vs_slot_count; ++index) {
        if (!can_read()) continue;
        const auto slot = plan.vs_slots[index];
        ComPtr<ID3D11ShaderResourceView> view; context->VSGetShaderResources(slot, 1, &view);
        if (!view) continue;
        ComPtr<ID3D11Resource> resource; view->GetResource(&resource);
        ComPtr<ID3D11Buffer> buffer;
        if (FAILED(resource.As(&buffer))) continue;
        D3D11_SHADER_RESOURCE_VIEW_DESC desc{}; view->GetDesc(&desc);
        D3D11_BUFFER_DESC b{}; buffer->GetDesc(&b);
        uint32_t stride = b.StructureByteStride;
        if (desc.Format == DXGI_FORMAT_R32G32B32A32_FLOAT) stride = 16;
        else if (desc.Format == DXGI_FORMAT_R32G32B32_FLOAT) stride = 12;
        else if (desc.Format == DXGI_FORMAT_R32G32_FLOAT) stride = 8;
        else if (desc.Format == DXGI_FORMAT_R32_FLOAT || desc.Format == DXGI_FORMAT_R32_TYPELESS) stride = 4;
        const auto element = desc.ViewDimension == D3D11_SRV_DIMENSION_BUFFEREX
            ? desc.BufferEx.FirstElement : desc.Buffer.FirstElement;
        const auto elements = desc.ViewDimension == D3D11_SRV_DIMENSION_BUFFEREX
            ? desc.BufferEx.NumElements : desc.Buffer.NumElements;
        if (!stride || element > b.ByteWidth / stride || elements > UINT32_MAX / stride) continue;
        const uint32_t offset = element * stride;
        auto data = read_buffer(buffer.Get(), offset, elements * stride);
        const auto bytes = static_cast<uint32_t>(data.size());
        const auto blob = data.empty() ? 0u : save_blob(capture, std::move(data));
        row.vs_buffers[row.vs_buffer_count++] = {
            uint64_t(uintptr_t(buffer.Get())), slot, uint32_t(desc.Format), offset, b.ByteWidth, bytes, blob,
        };
    }
    row.queued = queued_draw();
    if (!rsf_ac7_capture_record_geometry(capture, &plan, &row)) capture_failed(capture);
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
