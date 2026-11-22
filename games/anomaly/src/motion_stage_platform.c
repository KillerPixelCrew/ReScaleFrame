// SPDX-License-Identifier: MIT
// Native leaves of the motion stage (motion/stage.rs): the create_shader, G-buffer window and immediate-context draw
// detours, their unwind scopes, the SEH-guarded engine includer and the guarded engine reads. Rust owns the motion
// profile, guards, caller frame and the build's engine addresses (rsf_anomaly_motion_engine).
#include "motion_stage_platform.h"
#include "native_geometry.h"
#include "native_memory.h"
#include "native_observer.h"
#include "sr_stage.h"
#include "stage_entries.h"
#include <rescaleframe/game_frame.h>
#define COBJMACROS
#include <d3d11.h>
#include <d3dcompiler.h>
#include <intrin.h>
#include <string.h>

static uint8_t* image;
// The installed build's addresses. Until install every read fails on the null image.
static rsf_anomaly_motion_engine engine;

static bool read_global(uint32_t rva, void* value, size_t size)
{
    return image && guarded_copy(value, image + rva, size);
}

static rsf_anomaly_hook_set hooks;
static void* originals[RSF_ANOMALY_MOTION_SITE_COUNT];
static rsf_anomaly_hook_set draw_hooks;
static void* draw_originals[4];

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

static bool install_draw_hooks(void* context)
{
    void* targets[4] = {0};
    if (!rsf_anomaly_motion_resolve_draws(guarded_copy, context, &targets)) return false;
    void* const replacements[4] = {(void*)draw_indexed_hook, (void*)draw_hook, (void*)draw_indexed_instanced_hook,
        (void*)draw_instanced_hook};
    rsf_anomaly_hook_target sites[4];
    for (size_t i = 0; i < 4; ++i) {
        sites[i].address = targets[i];
        sites[i].replacement = replacements[i];
        sites[i].original = &draw_originals[i];
    }
    return rsf_anomaly_hooks_install_resolved(&draw_hooks, sites, 4);
}

static void* hw_device(void)
{
    void* device = NULL;
    (void)read_global(engine.hw_device_rva, &device, sizeof(device));
    return device;
}
static void* hw_context(void)
{
    void* context = NULL;
    (void)read_global(engine.hw_context_rva, &context, sizeof(context));
    return context;
}
static void* backend(void) { return image ? image + engine.backend_rva : NULL; }

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

static void flush(const rsf_anomaly_geometry_draw_facts* facts)
{
    const rsf_anomaly_motion_draw_facts draw = {facts->capture, facts->visual, facts->tree_lod, facts->pass,
        facts->kind, facts->verdict, facts->view, facts->domain, facts->in_visual, facts->detail};
    rsf_anomaly_motion_stage_flush(&draw);
}
static void observe_flush(bool enabled) { rsf_anomaly_geometry_set_flush(enabled ? flush : NULL); }

// Rust lends the profile's sites in RSF_ANOMALY_MOTION_SITE_* order; this leaf supplies the detours.
static bool install_hooks(void* module, uint32_t size, const rsf_anomaly_motion_engine* selected,
    const rsf_anomaly_hook_site* lent, size_t count)
{
    if (count != RSF_ANOMALY_MOTION_SITE_COUNT || !lent || !selected) return false;
    engine = *selected;
    image = (uint8_t*)module;
    if (rsf_anomaly_hooks_created(&hooks)) return rsf_anomaly_hooks_enable(&hooks);
    void* const detours[RSF_ANOMALY_MOTION_SITE_COUNT] = {(void*)create_shader_hook, (void*)scene_begin_hook,
        (void*)scene_end_hook};
    rsf_anomaly_hook_site sites[RSF_ANOMALY_MOTION_SITE_COUNT];
    for (size_t i = 0; i < RSF_ANOMALY_MOTION_SITE_COUNT; ++i) {
        sites[i] = lent[i];
        sites[i].replacement = detours[i];
        sites[i].original = &originals[i];
    }
    return rsf_anomaly_hooks_install(&hooks, image, size, sites, RSF_ANOMALY_MOTION_SITE_COUNT);
}
static bool disable_hooks(void) { return rsf_anomaly_hooks_disable(&hooks); }
static bool hooks_created(void) { return rsf_anomaly_hooks_created(&hooks); }

static void* render_target(void)
{
    void* target = NULL;
    return read_global(engine.render_target_rva, &target, sizeof(target)) ? target : NULL;
}
static bool position_views(void* target, rsf_anomaly_motion_position_pair* views)
{
    return rsf_anomaly_motion_position_views(guarded_copy, target, engine.position_offset, engine.crt_rtv_offset,
        engine.crt_surface_offset, views);
}
static bool native_frame(uint32_t* frame) { return read_global(engine.device_frame_rva, frame, sizeof(*frame)); }
static bool render_view(uint8_t* out)
{
    const uint8_t view = image ? rsf_anomaly_geometry_current_view(image, engine.build) : RSF_ANOMALY_GEOMETRY_UNSET;
    if (view == RSF_ANOMALY_GEOMETRY_UNSET) return false;
    *out = view;
    return true;
}
static uint64_t reset_generation(void)
{
    rsf_anomaly_native_observation observation;
    memset(&observation, 0, sizeof(observation));
    observation.struct_size = sizeof(observation);
    observation.abi_version = RSF_ANOMALY_OBSERVATION_ABI;
    return rsf_anomaly_observation(&observation) == RSF_OK ? observation.resets : 0;
}
static bool options_word3(uint32_t* options) { return read_global(engine.options_word3_rva, options, sizeof(*options)); }
static bool camera(float (*view)[16], float (*projection)[16])
{
    return image && guarded_copy(view, image + engine.view_rva, sizeof(*view)) &&
        guarded_copy(projection, image + engine.projection_rva, sizeof(*projection));
}
static bool world(float (*out)[16]) { return rsf_anomaly_geometry_world_matrix(*out); }
static void draw_state(void* context, rsf_anomaly_motion_draw_state* out)
{
    rsf_anomaly_motion_viewport(context, &out->viewport);
    rsf_anomaly_sr_jitter_uv jitter = {0};
    rsf_anomaly_sr_stage_jitter_uv(&jitter);
    out->jitter[0] = jitter.current_u;
    out->jitter[1] = jitter.current_v;
    out->jitter[2] = jitter.previous_u;
    out->jitter[3] = jitter.previous_v;
}

static uint32_t bind_read(void* host, const void* address, void* out, size_t size)
{
    (void)host;
    return guarded_copy(out, address, size);
}
static uint32_t bind_write(void* host, void* address, const void* in, size_t size)
{
    (void)host;
    return guarded_copy(address, in, size);
}
static const rsf_anomaly_motion_bind_platform bind_platform = {
    bind_read, bind_write, rsf_anomaly_motion_bind_targets, rsf_anomaly_motion_clear_targets,
};

// The engine's includer is lent as its vtable pointer; the object is that pointer's storage.
static bool include_open(void* table, const char* name, const void** data, uint32_t* size)
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
static void include_close(void* table, const void* data)
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
static bool include_copy(void* out, const void* in, size_t size) { return guarded_copy(out, in, size); }
static const rsf_anomaly_engine_include_ops include_ops = {include_open, include_copy, include_close};

static bool read_engine(const void* owner, uintptr_t address, uint8_t* out, size_t size)
{
    (void)owner;
    return guarded_copy(out, (const void*)address, size);
}
static bool copy_engine(void* out, const void* in, size_t size) { return guarded_copy(out, in, size); }

static const rsf_anomaly_motion_platform* motion_platform(void)
{
    static rsf_anomaly_motion_platform platform;
    static bool ready;
    if (!ready) {
        platform = (rsf_anomaly_motion_platform){
            .struct_size = sizeof(rsf_anomaly_motion_platform),
            .abi_version = RSF_ANOMALY_MOTION_PLATFORM_ABI,
            .device = rsf_anomaly_motion_device_platform_get(),
            .bind = &bind_platform,
            .copy = copy_engine,
            .geometry_active = rsf_anomaly_geometry_active,
            .observe_flush = observe_flush,
            .foliage_identity = rsf_anomaly_geometry_set_foliage,
            .install_hooks = install_hooks,
            .disable_hooks = disable_hooks,
            .hooks_created = hooks_created,
            .install_draw_hooks = install_draw_hooks,
            .render_target = render_target,
            .position_views = position_views,
            .native_frame = native_frame,
            .render_view = render_view,
            .reset_generation = reset_generation,
            .options_word3 = options_word3,
            .target_facts = rsf_anomaly_motion_target_facts,
            .backend = backend,
            .context = hw_context,
            .shader_device = hw_device,
            .clear_target = rsf_anomaly_motion_clear_target,
            .bound_shaders = rsf_anomaly_motion_bound_shaders,
            .set_shaders = rsf_anomaly_motion_set_shaders,
            .replay = rsf_anomaly_motion_replay_platform_get(),
            .camera = camera,
            .world = world,
            .read_constant = rsf_anomaly_geometry_read_constants,
            .write_constant = rsf_anomaly_geometry_write_constants,
            .draw_state = draw_state,
            .includes = &include_ops,
            .read_engine = read_engine,
        };
        ready = true;
    }
    return &platform;
}

_Static_assert(sizeof(rsf_anomaly_motion_position_pair) == 16 && sizeof(rsf_anomaly_motion_target_info) == 16 &&
    sizeof(rsf_anomaly_motion_draw_facts) == 32, "motion stage records");
_Static_assert(sizeof(rsf_anomaly_motion_shader_objects) == 40, "shader objects");
_Static_assert(sizeof(rsf_anomaly_motion_shader_creation) == 56 &&
    offsetof(rsf_anomaly_motion_shader_creation, return_address) == 32 &&
    offsetof(rsf_anomaly_motion_shader_creation, status) == 48, "shader creation");
_Static_assert(sizeof(rsf_anomaly_engine_include_ops) == 24 && offsetof(rsf_anomaly_engine_include_ops, close) == 16,
    "include ops");
_Static_assert(sizeof(rsf_anomaly_motion_engine) == 56 && offsetof(rsf_anomaly_motion_engine, hw_device_rva) == 8 &&
    offsetof(rsf_anomaly_motion_engine, projection_rva) == 48, "motion engine");
_Static_assert(sizeof(rsf_anomaly_motion_platform) == 256 && offsetof(rsf_anomaly_motion_platform, copy) == 24 &&
    offsetof(rsf_anomaly_motion_platform, install_hooks) == 56 &&
    offsetof(rsf_anomaly_motion_platform, render_target) == 88 &&
    offsetof(rsf_anomaly_motion_platform, target_facts) == 136 &&
    offsetof(rsf_anomaly_motion_platform, includes) == 240 &&
    offsetof(rsf_anomaly_motion_platform, read_engine) == 248, "motion platform");
_Static_assert(sizeof(rsf_anomaly_motion_frame_target) == 24 && offsetof(rsf_anomaly_motion_frame_target, written) == 20 &&
    offsetof(rsf_anomaly_motion_frame_target, main) == 21, "frame target");
_Static_assert(RSF_PHASE_RENDER_SUBMIT == 2 && RSF_DUMP_VIEW_DENSE_MOTION == 7, "frame phase and dump view");

// C entries for the bootstrap and observer leaves (stage_entries.h).
void rsf_anomaly_motion_configure_native(const wchar_t* ini, rsf_game_log_fn log)
{
    rsf_anomaly_motion_stage_configure(motion_platform(), ini, log, NULL);
}
bool rsf_anomaly_motion_install_native(void* module, size_t build)
{
    return rsf_anomaly_motion_stage_install(motion_platform(), module, build);
}
