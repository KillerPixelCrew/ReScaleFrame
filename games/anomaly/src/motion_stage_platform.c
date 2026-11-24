// SPDX-License-Identifier: MIT
// Native leaves of the motion stage (motion/stage.rs): the create_shader, G-buffer window and immediate-context draw
// detours, their unwind scopes and the SEH-guarded engine includer. Rust owns the motion profile, guards, caller frame,
// the engine reads and the hook sets (motion/stage/native.rs).
#include "motion_stage_platform.h"
#include "native_geometry.h"
#include "native_memory.h"
#include <rescaleframe/game_frame.h>
#define COBJMACROS
#include <d3d11.h>
#include <d3dcompiler.h>
#include <intrin.h>
#include <string.h>

// MinHook writes the trampolines of the detours below (motion/stage/native.rs installs them).
void* rsf_anomaly_motion_originals[RSF_ANOMALY_MOTION_SITE_COUNT];
void* rsf_anomaly_motion_draw_originals[4];
#define originals rsf_anomaly_motion_originals
#define draw_originals rsf_anomaly_motion_draw_originals

typedef void (*unary_fn)(void*);
typedef long long (*create_shader_fn)(const char*, const void*, unsigned, const char*, void**, char);
typedef void (STDMETHODCALLTYPE* draw_indexed_fn)(void*, UINT, UINT, INT);
typedef void (STDMETHODCALLTYPE* draw_fn)(void*, UINT, UINT);
typedef void (STDMETHODCALLTYPE* draw_indexed_instanced_fn)(void*, UINT, UINT, UINT, INT, UINT);
typedef void (STDMETHODCALLTYPE* draw_instanced_fn)(void*, UINT, UINT, UINT, UINT);

_Static_assert(sizeof(rsf_anomaly_motion_draw_scope) == 176 && _Alignof(rsf_anomaly_motion_draw_scope) == 8 &&
    sizeof(rsf_anomaly_motion_replay_scope) == 2184 && _Alignof(rsf_anomaly_motion_replay_scope) == 8,
    "motion draw and replay scopes");
_Static_assert(RSF_ANOMALY_MOTION_NO_DRAW == 0 && RSF_ANOMALY_MOTION_RESTORE_DRAW == 1 &&
    RSF_ANOMALY_MOTION_REPLAY_DRAW == 2, "motion draw routes");

// One immediate-context draw, replayed with the same arguments.
enum { draw_indexed, draw_plain, draw_indexed_instanced, draw_instanced };
typedef struct draw_call {
    uint32_t kind;
    void* context;
    UINT count, instances, start, first;
    INT base;
} draw_call;

static void call_draw(const draw_call* call)
{
    void* const original = draw_originals[call->kind];
    switch (call->kind) {
    case draw_indexed:
        ((draw_indexed_fn)original)(call->context, call->count, call->start, call->base);
        break;
    case draw_plain:
        ((draw_fn)original)(call->context, call->count, call->start);
        break;
    case draw_indexed_instanced:
        ((draw_indexed_instanced_fn)original)(call->context, call->count, call->instances, call->start, call->base,
            call->first);
        break;
    default:
        ((draw_instanced_fn)original)(call->context, call->count, call->instances, call->start, call->first);
        break;
    }
}

// The engine's draw and the replay may unwind through C++. Each scope is retired on return and during that unwind:
// by the termination handler under MSVC, by the cleanup handler under GCC (built with -fexceptions).
#ifdef _MSC_VER
static void replay_draw(const rsf_anomaly_motion_draw_scope* scope, const draw_call* call)
{
    rsf_anomaly_motion_replay_scope replay;
    if (!rsf_anomaly_motion_stage_replay_begin(scope, &replay)) return;
    volatile bool completed = false;
    RSF_FOREIGN_TRY {
        call_draw(call);
        completed = true;
    } RSF_FOREIGN_FINALLY {
        rsf_anomaly_motion_stage_replay_end(scope, &replay, completed);
    }
}

static void around_draw(const draw_call* call)
{
    rsf_anomaly_motion_draw_scope scope;
    const uint8_t route = rsf_anomaly_motion_stage_draw_begin(call->context, &scope);
    volatile bool completed = false;
    RSF_FOREIGN_TRY {
        call_draw(call);
        completed = true;
        if (route == RSF_ANOMALY_MOTION_REPLAY_DRAW) replay_draw(&scope, call);
    } RSF_FOREIGN_FINALLY {
        if (route != RSF_ANOMALY_MOTION_NO_DRAW) rsf_anomaly_motion_stage_draw_end(&scope, completed);
    }
}
#else
typedef struct replay_guard {
    const rsf_anomaly_motion_draw_scope* scope;
    rsf_anomaly_motion_replay_scope* replay;
    bool completed;
} replay_guard;
static void end_replay(replay_guard* guard)
{
    rsf_anomaly_motion_stage_replay_end(guard->scope, guard->replay, guard->completed);
}
static void replay_draw(const rsf_anomaly_motion_draw_scope* scope, const draw_call* call)
{
    rsf_anomaly_motion_replay_scope replay;
    if (!rsf_anomaly_motion_stage_replay_begin(scope, &replay)) return;
    replay_guard guard __attribute__((cleanup(end_replay))) = {scope, &replay, false};
    call_draw(call);
    guard.completed = true;
}

typedef struct draw_guard {
    rsf_anomaly_motion_draw_scope* scope;
    bool initialized;
    bool completed;
} draw_guard;
static void end_draw(draw_guard* guard)
{
    if (guard->initialized) rsf_anomaly_motion_stage_draw_end(guard->scope, guard->completed);
}
static void around_draw(const draw_call* call)
{
    rsf_anomaly_motion_draw_scope scope;
    const uint8_t route = rsf_anomaly_motion_stage_draw_begin(call->context, &scope);
    draw_guard guard __attribute__((cleanup(end_draw))) = {&scope, route != RSF_ANOMALY_MOTION_NO_DRAW, false};
    call_draw(call);
    guard.completed = true;
    if (route == RSF_ANOMALY_MOTION_REPLAY_DRAW) replay_draw(&scope, call);
}
#endif

static void STDMETHODCALLTYPE draw_indexed_hook(void* context, UINT count, UINT start, INT base)
{
    const draw_call call = {draw_indexed, context, count, 0, start, 0, base};
    around_draw(&call);
}
static void STDMETHODCALLTYPE draw_hook(void* context, UINT count, UINT start)
{
    const draw_call call = {draw_plain, context, count, 0, start, 0, 0};
    around_draw(&call);
}
static void STDMETHODCALLTYPE draw_indexed_instanced_hook(void* context, UINT count, UINT instances, UINT start,
    INT base, UINT first)
{
    const draw_call call = {draw_indexed_instanced, context, count, instances, start, first, base};
    around_draw(&call);
}
static void STDMETHODCALLTYPE draw_instanced_hook(void* context, UINT count, UINT instances, UINT start, UINT first)
{
    const draw_call call = {draw_instanced, context, count, instances, start, first, 0};
    around_draw(&call);
}

// In the order of the context's DrawIndexed, Draw, DrawIndexedInstanced and DrawInstanced.
void* const rsf_anomaly_motion_draw_detours[4] = {
    (void*)&draw_indexed_hook, (void*)&draw_hook, (void*)&draw_indexed_instanced_hook, (void*)&draw_instanced_hook,
};
static long long create_shader_hook(const char* target, const void* buffer, unsigned size, const char* file,
    void** result, char disasm)
{
    rsf_anomaly_motion_stage_attach();
    const long long hr = ((create_shader_fn)originals[RSF_ANOMALY_MOTION_SITE_CREATE_SHADER])(
        target, buffer, size, file, result, disasm);
    // The caller's rsp at the call is the return-address slot + 8; Rust decodes shader_compile's frame from it.
    const rsf_anomaly_motion_shader_creation creation = {target, buffer, file, (void* const*)result,
        _ReturnAddress(), (const uint8_t*)_AddressOfReturnAddress() + 8, (int32_t)hr, size};
    rsf_anomaly_motion_stage_shader(&creation);
    return hr;
}
static void scene_begin_hook(void* self)
{
    rsf_anomaly_motion_stage_cancel_draw();
    ((unary_fn)originals[RSF_ANOMALY_MOTION_SITE_PHASE_SCENE_BEGIN])(self);
    rsf_anomaly_motion_stage_open(self);
}
static void scene_end_hook(void* self)
{
    rsf_anomaly_motion_stage_close();
    ((unary_fn)originals[RSF_ANOMALY_MOTION_SITE_PHASE_SCENE_END])(self);
}

// In RSF_ANOMALY_MOTION_SITE_* order.
void* const rsf_anomaly_motion_detours[RSF_ANOMALY_MOTION_SITE_COUNT] = {
    (void*)&create_shader_hook, (void*)&scene_begin_hook, (void*)&scene_end_hook,
};
// The engine's includer is lent as its vtable pointer; the object is that pointer's storage.
bool rsf_anomaly_motion_native_include_open(void* table, const char* name, const void** data, uint32_t* size)
{
    ID3DInclude* object = (ID3DInclude*)&table;
#ifdef _MSC_VER
    RSF_FOREIGN_TRY {
        return SUCCEEDED(object->lpVtbl->Open(object, D3D_INCLUDE_LOCAL, name, NULL, data, size));
    } RSF_FAULT_CATCH {
        return false;
    }
#else
    return SUCCEEDED(object->lpVtbl->Open(object, D3D_INCLUDE_LOCAL, name, NULL, data, size));
#endif
}
void rsf_anomaly_motion_native_include_close(void* table, const void* data)
{
    ID3DInclude* object = (ID3DInclude*)&table;
#ifdef _MSC_VER
    RSF_FOREIGN_TRY {
        object->lpVtbl->Close(object, data);
    } RSF_FAULT_CATCH {
    }
#else
    object->lpVtbl->Close(object, data);
#endif
}

_Static_assert(sizeof(rsf_anomaly_motion_shader_objects) == 40, "shader objects");
_Static_assert(sizeof(rsf_anomaly_motion_shader_creation) == 56 &&
    offsetof(rsf_anomaly_motion_shader_creation, return_address) == 32 &&
    offsetof(rsf_anomaly_motion_shader_creation, status) == 48, "shader creation");

_Static_assert(sizeof(rsf_anomaly_motion_frame_target) == 24 && offsetof(rsf_anomaly_motion_frame_target, written) == 20 &&
    offsetof(rsf_anomaly_motion_frame_target, main) == 21, "frame target");
_Static_assert(RSF_PHASE_RENDER_SUBMIT == 2 && RSF_DUMP_VIEW_DENSE_MOTION == 7, "frame phase and dump view");

