/* SPDX-License-Identifier: MIT */
/* Typed Unreal detours and guarded engine calls for the F9 capture. Only Rust interprets the
   numeric observations. This leaf keeps what Rust cannot express: SEH around a raw engine call,
   and RSF_FOREIGN_TRY/RSF_FOREIGN_FINALLY termination handlers that restore the Rust-owned engine and root scopes
   when an original call unwinds, as the former C++ scope destructors did. Without MSVC the
   calls are unguarded, as they were in the C++ build. */
#include <rescaleframe/foreign_guard.h>
#include <rescaleframe/ac7_hooks.h>
#include <rescaleframe/ac7_memory.h>
#include <stdalign.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#if defined(_MSC_VER)
/* MSVC unwinds through the detour when an original call raises. The restore must still run. */
#define RSF_SCOPE_TRY RSF_FOREIGN_TRY {
#define RSF_SCOPE_FINALLY(cleanup) } RSF_FOREIGN_FINALLY { cleanup; }
#else
#define RSF_SCOPE_TRY {
#define RSF_SCOPE_FINALLY(cleanup) } cleanup;
#endif

/* The Rust record assertions document the sizes and lifetimes of this inline storage. */
typedef struct EngineRecord { alignas(8) unsigned char bytes[264]; } EngineRecord;
typedef struct RootRecord { alignas(8) unsigned char bytes[376]; } RootRecord;
typedef struct NativeRootScope { uint64_t id, view; } NativeRootScope;
typedef struct MotionCaptureInterval { uint32_t session, interval; } MotionCaptureInterval;
_Static_assert(sizeof(EngineRecord) == 264 && alignof(EngineRecord) == 8, "engine record");
_Static_assert(sizeof(RootRecord) == 376 && alignof(RootRecord) == 8, "root record");
_Static_assert(sizeof(NativeRootScope) == 16 && sizeof(MotionCaptureInterval) == 8, "scope");

enum {
    POSTPROCESS_BEGIN = 0, WIDGET_PREPARE_BEFORE = 2, WIDGET_QUEUE_BEFORE = 4,
    MAIN_TEMPORAL_BUILD_BEFORE = 6, MAIN_TEMPORAL_PROCESS_BEGIN = 8, GRAPH_PASS_BEGIN = 10,
};

void rsf_ac7_capture_fail(MotionCaptureInterval);
MotionCaptureInterval rsf_ac7_capture_interval(void);
MotionCaptureInterval rsf_ac7_capture_sample(void);
EngineRecord* rsf_ac7_capture_swap_engine(EngineRecord*);
void rsf_ac7_capture_begin_engine(EngineRecord*, MotionCaptureInterval, uint64_t, uint64_t, bool);
void rsf_ac7_capture_history(uint64_t, bool, const float*, bool (*)(float*, const float*));
void rsf_ac7_capture_has_velocity(uint64_t, bool);
bool rsf_ac7_capture_record_engine(MotionCaptureInterval, const EngineRecord*, bool);
void rsf_ac7_capture_root_before(RootRecord*, MotionCaptureInterval, uint32_t, uint64_t, uint64_t,
    uint64_t, uint64_t);
void rsf_ac7_capture_root_after(RootRecord*, MotionCaptureInterval);
NativeRootScope rsf_ac7_capture_enter_root(const RootRecord*);
void rsf_ac7_capture_restore_root(const NativeRootScope*);
bool rsf_ac7_capture_get_graph_output(uint64_t getter, uint64_t node, uint32_t index, uint64_t* output);

static bool active(MotionCaptureInterval capture)
{
    return capture.interval != UINT32_MAX;
}

static uint64_t address(const void* object)
{
    return (uint64_t)(uintptr_t)object;
}

typedef void* (*get_output_fn)(void*, uint32_t);

bool rsf_ac7_capture_get_graph_output(uint64_t getter, uint64_t node, uint32_t index, uint64_t* output)
{
    bool read = false;
    if (!getter) return false;
#if defined(_MSC_VER)
    RSF_FOREIGN_TRY {
#endif
        const get_output_fn call = (get_output_fn)(uintptr_t)getter;
        *output = address(call((void*)(uintptr_t)node, index));
        read = true;
#if defined(_MSC_VER)
    } RSF_FAULT_CATCH {
        read = false;
    }
#endif
    return read;
}

typedef bool (*should_fn)(void*, void*, bool);
typedef bool (*has_fn)(void*, void*);
typedef bool (*history_fn)(void*, void*, float*);
typedef void (*postprocess_fn)(void*, void*, void*, void*);
typedef void (*widget_prepare_fn)(void*);
typedef void (*widget_queue_fn)(void*, float);
typedef void (*add_temporal_fn)(void*, void*);
typedef void (*temporal_process_fn)(void*, void*);
typedef void (*graph_process_fn)(void*, void*, void*);

static should_fn original_should;
static has_fn original_has;
static history_fn original_history;
static postprocess_fn original_postprocess;
static widget_prepare_fn original_widget_prepare;
static widget_queue_fn original_widget_queue;
static add_temporal_fn original_add_temporal;
static temporal_process_fn original_temporal_process;
static graph_process_fn original_graph_process;

static bool copy_previous(float* destination, const float* source)
{
    return rsf_ac7_copy_memory(destination, source, 16 * sizeof(float));
}

static bool hooked_history(void* cache, void* primitive, float* previous)
{
    const bool found = original_history(cache, primitive, previous);
    rsf_ac7_capture_history(address(primitive), found, previous, copy_previous);
    return found;
}

static bool hooked_has(void* view, void* primitive)
{
    const bool has_velocity = original_has(view, primitive);
    rsf_ac7_capture_has_velocity(address(primitive), has_velocity);
    return has_velocity;
}

static bool hooked_should(void* primitive, void* view, bool check_visibility)
{
    const MotionCaptureInterval capture = rsf_ac7_capture_interval();
    EngineRecord event = {0};
    EngineRecord* previous;
    bool accepted = false;
    if (!active(capture)) return original_should(primitive, view, check_visibility);
    rsf_ac7_capture_begin_engine(&event, capture, address(primitive), address(view), check_visibility);
    previous = rsf_ac7_capture_swap_engine(&event);
    RSF_SCOPE_TRY
        accepted = original_should(primitive, view, check_visibility);
    RSF_SCOPE_FINALLY(rsf_ac7_capture_swap_engine(previous))
    if (!rsf_ac7_capture_record_engine(capture, &event, accepted)) rsf_ac7_capture_fail(capture);
    return accepted;
}

/* The original-call arguments of one root detour. */
typedef struct RootCall {
    uint32_t kind;
    void* owner;
    void* first;
    void* second;
    void* third;
    float delta;
} RootCall;

static void call_original(const RootCall* call)
{
    switch (call->kind) {
    case POSTPROCESS_BEGIN: original_postprocess(call->owner, call->first, call->second, call->third); break;
    case WIDGET_PREPARE_BEFORE: original_widget_prepare(call->owner); break;
    case WIDGET_QUEUE_BEFORE: original_widget_queue(call->owner, call->delta); break;
    case MAIN_TEMPORAL_BUILD_BEFORE: original_add_temporal(call->owner, call->first); break;
    case MAIN_TEMPORAL_PROCESS_BEGIN: original_temporal_process(call->owner, call->first); break;
    case GRAPH_PASS_BEGIN: original_graph_process(call->owner, call->first, call->second); break;
    default: break;
    }
}

static void observe_root(MotionCaptureInterval capture, const RootCall* call)
{
    RootRecord record = {0};
    NativeRootScope previous;
    if (!active(capture)) {
        call_original(call);
        return;
    }
    rsf_ac7_capture_root_before(&record, capture, call->kind, address(call->owner),
        address(call->first), address(call->second), address(call->third));
    previous = rsf_ac7_capture_enter_root(&record);
    RSF_SCOPE_TRY
        call_original(call);
    RSF_SCOPE_FINALLY(rsf_ac7_capture_restore_root(&previous))
    rsf_ac7_capture_root_after(&record, capture);
}

static void hooked_postprocess(void* owner, void* commands, void* view, void* velocity)
{
    const RootCall call = { POSTPROCESS_BEGIN, owner, commands, view, velocity, 0.0f };
    observe_root(rsf_ac7_capture_interval(), &call);
}

static void hooked_widget_prepare(void* converter)
{
    const RootCall call = { WIDGET_PREPARE_BEFORE, converter, NULL, NULL, NULL, 0.0f };
    observe_root(rsf_ac7_capture_interval(), &call);
}

static void hooked_widget_queue(void* converter, float delta)
{
    const RootCall call = { WIDGET_QUEUE_BEFORE, converter, NULL, NULL, NULL, delta };
    observe_root(rsf_ac7_capture_interval(), &call);
}

static void hooked_add_temporal(void* context, void* velocity)
{
    const RootCall call = { MAIN_TEMPORAL_BUILD_BEFORE, context, velocity, NULL, NULL, 0.0f };
    observe_root(rsf_ac7_capture_interval(), &call);
}

static void hooked_temporal_process(void* node, void* context)
{
    const RootCall call = { MAIN_TEMPORAL_PROCESS_BEGIN, node, context, NULL, NULL, 0.0f };
    observe_root(rsf_ac7_capture_interval(), &call);
}

static void hooked_graph_process(void* graph, void* output, void* context)
{
    const RootCall call = { GRAPH_PASS_BEGIN, graph, output, context, NULL, 0.0f };
    observe_root(rsf_ac7_capture_sample(), &call);
}

#define RSF_COUNT(array) ((uint32_t)(sizeof(array) / sizeof((array)[0])))

/* Order matches the Rust capture's guarded root sites. */
bool rsf_ac7_capture_install_root_batch(void** targets, uint32_t count)
{
    void* const detours[] = { (void*)&hooked_postprocess, (void*)&hooked_widget_prepare,
        (void*)&hooked_widget_queue, (void*)&hooked_add_temporal, (void*)&hooked_temporal_process,
        (void*)&hooked_graph_process };
    void** const originals[] = { (void**)&original_postprocess, (void**)&original_widget_prepare,
        (void**)&original_widget_queue, (void**)&original_add_temporal,
        (void**)&original_temporal_process, (void**)&original_graph_process };
    return count == RSF_COUNT(detours)
        && rsf_ac7_capture_install_hooks(targets, detours, originals, count);
}

bool rsf_ac7_capture_install_engine_batch(void** targets, uint32_t count)
{
    void* const detours[] = { (void*)&hooked_should, (void*)&hooked_has, (void*)&hooked_history };
    void** const originals[] = { (void**)&original_should, (void**)&original_has,
        (void**)&original_history };
    return count == RSF_COUNT(detours)
        && rsf_ac7_capture_install_hooks(targets, detours, originals, count);
}
