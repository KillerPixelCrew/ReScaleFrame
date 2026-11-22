// SPDX-License-Identifier: MIT
#include <rescaleframe/ac7_hooks.h>
#include <rescaleframe/ac7_memory.h>
#include <rescaleframe/ac7_motion_capture.h>
#include <cstddef>
#include <windows.h>
#include <iterator>

// The Rust record assertions document the sizes and lifetimes of this inline storage. Only
// Rust interprets the numeric observations. C++ retains the original-call unwind boundary.
struct alignas(8) EngineRecord { std::byte bytes[264]{}; };
struct alignas(8) RootRecord { std::byte bytes[376]{}; };
struct NativeRootScope { uint64_t id, view; };
struct MotionCaptureInterval {
    uint32_t session, interval;
    explicit operator bool() const noexcept { return interval != UINT32_MAX; }
};
static_assert(sizeof(EngineRecord) == 264 && alignof(EngineRecord) == 8);
static_assert(sizeof(RootRecord) == 376 && alignof(RootRecord) == 8);
static_assert(sizeof(NativeRootScope) == 16 && sizeof(MotionCaptureInterval) == 8);
enum RootKind : uint32_t {
    postprocess_begin = 0, widget_prepare_before = 2, widget_queue_before = 4,
    main_temporal_build_before = 6, main_temporal_process_begin = 8, graph_pass_begin = 10,
};
extern "C" {
void rsf_ac7_capture_fail(MotionCaptureInterval);
MotionCaptureInterval rsf_ac7_capture_interval();
MotionCaptureInterval rsf_ac7_capture_sample();
EngineRecord* rsf_ac7_capture_swap_engine(EngineRecord*);
void rsf_ac7_capture_begin_engine(EngineRecord*, MotionCaptureInterval, uint64_t, uint64_t, bool);
void rsf_ac7_capture_history(uint64_t, bool, const float*, bool(*)(float*, const float*));
void rsf_ac7_capture_has_velocity(uint64_t, bool);
bool rsf_ac7_capture_record_engine(MotionCaptureInterval, const EngineRecord*, bool);

void rsf_ac7_capture_root_before(RootRecord*, MotionCaptureInterval, RootKind, uint64_t, uint64_t, uint64_t, uint64_t);
void rsf_ac7_capture_root_after(RootRecord*, MotionCaptureInterval);
NativeRootScope rsf_ac7_capture_enter_root(const RootRecord*);
void rsf_ac7_capture_restore_root(const NativeRootScope*);
}

extern "C" bool rsf_ac7_capture_get_graph_output(uint64_t getter, uint64_t node, uint32_t index, uint64_t* output)
{
    using GetOutput = void*(*)(void*, uint32_t);
    if (!getter) return false;
    const auto call = reinterpret_cast<GetOutput>(uintptr_t(getter));
#if defined(_MSC_VER)
    __try {
#endif
        *output = uint64_t(uintptr_t(call(reinterpret_cast<void*>(uintptr_t(node)), index)));
        return true;
#if defined(_MSC_VER)
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#endif
}

namespace {
bool (*original_should)(void*, void*, bool) = nullptr;
bool (*original_has)(void*, void*) = nullptr;
bool (*original_history)(void*, void*, float*) = nullptr;
void (*original_postprocess)(void*, void*, void*, void*) = nullptr;
void (*original_widget_prepare)(void*) = nullptr;
void (*original_widget_queue)(void*, float) = nullptr;
void (*original_add_temporal)(void*, void*) = nullptr;
void (*original_temporal_process)(void*, void*) = nullptr;
void (*original_graph_process)(void*, void*, void*) = nullptr;

uint64_t address(void* object) { return uint64_t(uintptr_t(object)); }

struct EngineScope {
    EngineRecord* previous;
    ~EngineScope() { rsf_ac7_capture_swap_engine(previous); }
};
struct RootScope {
    NativeRootScope previous;
    ~RootScope() { rsf_ac7_capture_restore_root(&previous); }
};

bool copy_previous(float* destination, const float* source) noexcept
{
    return rsf_ac7_copy_memory(destination, source, 16 * sizeof(float));
}

bool hooked_history(void* cache, void* primitive, float* previous)
{
    const bool found = original_history(cache, primitive, previous);
    rsf_ac7_capture_history(address(primitive), found, previous, copy_previous);
    return found;
}

bool hooked_has(void* view, void* primitive)
{
    const bool has_velocity = original_has(view, primitive);
    rsf_ac7_capture_has_velocity(address(primitive), has_velocity);
    return has_velocity;
}

bool hooked_should(void* primitive, void* view, bool check_visibility)
{
    const auto capture = rsf_ac7_capture_interval();
    if (!capture) return original_should(primitive, view, check_visibility);
    EngineRecord event;
    rsf_ac7_capture_begin_engine(&event, capture, address(primitive), address(view), check_visibility);
    bool accepted;
    {
        const EngineScope scope{rsf_ac7_capture_swap_engine(&event)};
        accepted = original_should(primitive, view, check_visibility);
    }
    if (!rsf_ac7_capture_record_engine(capture, &event, accepted)) rsf_ac7_capture_fail(capture);
    return accepted;
}

template<class Original>
void observe_root(MotionCaptureInterval capture, RootKind kind, Original call,
                  void* owner, void* first = nullptr, void* second = nullptr, void* third = nullptr)
{
    if (!capture) { call(); return; }
    RootRecord record;
    rsf_ac7_capture_root_before(&record, capture, kind, address(owner), address(first), address(second), address(third));
    {
        const RootScope scope{rsf_ac7_capture_enter_root(&record)};
        call();
    }
    rsf_ac7_capture_root_after(&record, capture);
}

void hooked_postprocess(void* owner, void* commands, void* view, void* velocity)
{
    observe_root(rsf_ac7_capture_interval(), postprocess_begin,
        [&] { original_postprocess(owner, commands, view, velocity); }, owner, commands, view, velocity);
}
void hooked_widget_prepare(void* converter)
{
    observe_root(rsf_ac7_capture_interval(), widget_prepare_before,
        [&] { original_widget_prepare(converter); }, converter);
}
void hooked_widget_queue(void* converter, float delta)
{
    observe_root(rsf_ac7_capture_interval(), widget_queue_before,
        [&] { original_widget_queue(converter, delta); }, converter);
}
void hooked_add_temporal(void* context, void* velocity)
{
    observe_root(rsf_ac7_capture_interval(), main_temporal_build_before,
        [&] { original_add_temporal(context, velocity); }, context, velocity);
}
void hooked_temporal_process(void* node, void* context)
{
    observe_root(rsf_ac7_capture_interval(), main_temporal_process_begin,
        [&] { original_temporal_process(node, context); }, node, context);
}
void hooked_graph_process(void* graph, void* output, void* context)
{
    observe_root(rsf_ac7_capture_sample(), graph_pass_begin,
        [&] { original_graph_process(graph, output, context); }, graph, output, context);
}

bool install_root_hooks(void** targets, uint32_t count)
{
    // Order matches the Rust capture's guarded root sites.
    void* detours[] = {reinterpret_cast<void*>(&hooked_postprocess), reinterpret_cast<void*>(&hooked_widget_prepare),
        reinterpret_cast<void*>(&hooked_widget_queue), reinterpret_cast<void*>(&hooked_add_temporal),
        reinterpret_cast<void*>(&hooked_temporal_process), reinterpret_cast<void*>(&hooked_graph_process)};
    void** originals[] = {reinterpret_cast<void**>(&original_postprocess),
        reinterpret_cast<void**>(&original_widget_prepare), reinterpret_cast<void**>(&original_widget_queue),
        reinterpret_cast<void**>(&original_add_temporal), reinterpret_cast<void**>(&original_temporal_process),
        reinterpret_cast<void**>(&original_graph_process)};
    return count == std::size(detours) && rsf_ac7_capture_install_hooks(targets, detours, originals, count);
}

bool install_engine_hooks(void** targets, uint32_t count)
{
    void* detours[] = {reinterpret_cast<void*>(&hooked_should), reinterpret_cast<void*>(&hooked_has),
        reinterpret_cast<void*>(&hooked_history)};
    void** originals[] = {reinterpret_cast<void**>(&original_should), reinterpret_cast<void**>(&original_has),
        reinterpret_cast<void**>(&original_history)};
    return count == std::size(detours) && rsf_ac7_capture_install_hooks(targets, detours, originals, count);
}

} // namespace

extern "C" bool rsf_ac7_capture_install_root_batch(void** targets, uint32_t count)
{
    return install_root_hooks(targets, count);
}

extern "C" bool rsf_ac7_capture_install_engine_batch(void** targets, uint32_t count)
{
    return install_engine_hooks(targets, count);
}
