// SPDX-License-Identifier: MIT
// Native observer detours, typed trampoline calls and fault boundaries for the Rust observer (native_observer.rs).
#include "native_observer.h"
#include "engine_slots.h"
#include "fg_stage_platform.h"
#include "input_hooks.h"
#include "native_hooks.h"
#include "native_memory.h"
#include "stage_entries.h"

// native_observer.rs.
const char* rsf_anomaly_observer_identify(rsf_anomaly_observer_engine*, const rsf_anomaly_image_headers*,
    rsf_anomaly_guard_read_fn);
bool rsf_anomaly_observer_before(const rsf_anomaly_observer_platform*, uint32_t boundary);
void rsf_anomaly_observer_after(const rsf_anomaly_observer_platform*, uint32_t boundary,
    const float* view, const float* projection, uint32_t target);
void rsf_anomaly_observer_leave(void);
typedef bool (*rsf_anomaly_observer_install_fn)(void*, uint32_t, const rsf_anomaly_hook_site*, size_t);
bool rsf_anomaly_observer_install(const rsf_anomaly_observer_engine*, const rsf_anomaly_observer_binding*,
    rsf_anomaly_observer_install_fn);
bool rsf_anomaly_observer_read_frame(const rsf_anomaly_observer_engine*, rsf_anomaly_guard_read_fn,
    rsf_anomaly_observer_frame*);

_Static_assert(sizeof(rsf_anomaly_observer_binding) == 16 && offsetof(rsf_anomaly_observer_binding, original) == 8,
    "observer binding layout");

// Same order as native_sites in both exact-build records (native_profile.rs).
enum { observer_site_count = RSF_ANOMALY_OBSERVER_RESET_END + 1 };
_Static_assert(observer_site_count == 8, "observer site count");

typedef void (*unary_fn)(void*);
typedef void (*camera_fn)(void*, const float*, const float*);
typedef void (*copy_target_fn)(void*, uint32_t);

// MinHook writes each trampoline to untyped storage. Cast only at the native call site.
static void* trampolines[observer_site_count];
static rsf_anomaly_hook_set hooks;

// One detour invocation. Rust admits it before the original and records completion facts after it. Begin, End,
// camera and target-copy facts follow their original calls; the other boundaries report before them.
typedef struct invocation {
    uint32_t boundary;
    void* self;
    const float* view;
    const float* projection;
    uint32_t target;
} invocation;

static const char* identify(rsf_anomaly_observer_engine*);
static bool enable_hooks(const rsf_anomaly_observer_engine*);
static bool disable_hooks(void);
static bool install_geometry(const rsf_anomaly_observer_engine*);
static bool install_sr(const rsf_anomaly_observer_engine*);
static bool install_motion(const rsf_anomaly_observer_engine*);
static bool enable_input(void);
static bool disable_input(void);
static bool read_frame(const rsf_anomaly_observer_engine*, rsf_anomaly_observer_frame*);

static const rsf_anomaly_observer_platform platform = {
    .identify = identify,
    .enable_hooks = enable_hooks,
    .disable_hooks = disable_hooks,
    .install_geometry = install_geometry,
    .disable_geometry = rsf_anomaly_geometry_disable_native,
    .install_sr = install_sr,
    .quiesce_sr = rsf_anomaly_sr_stage_quiesce,
    .install_motion = install_motion,
    .quiesce_motion = rsf_anomaly_motion_stage_quiesce,
    .enable_input = enable_input,
    .disable_input = disable_input,
    .input_idle = rsf_anomaly_input_idle,
    .read_frame = read_frame,
    .guarded_copy = guarded_copy,
    .render_begin = rsf_anomaly_fg_stage_render_begin,
    .sr_frame_end = rsf_anomaly_sr_stage_frame_end,
    .end_before = rsf_anomaly_fg_stage_end_before,
    .end_after = rsf_anomaly_fg_stage_end_after,
    .camera = rsf_anomaly_fg_stage_camera,
    .ui_boundary = rsf_anomaly_fg_stage_ui_boundary,
    .scope_frame = rsf_anomaly_fg_stage_scope,
    .device_reset = rsf_anomaly_fg_stage_device_reset,
    .log = guarded_log_line,
};

static void call_original(const invocation* call)
{
    void* const original = trampolines[call->boundary];
    switch (call->boundary) {
    case RSF_ANOMALY_OBSERVER_CAMERA:
        ((camera_fn)original)(call->self, call->view, call->projection);
        break;
    case RSF_ANOMALY_OBSERVER_TARGET_COPY:
        ((copy_target_fn)original)(call->self, call->target);
        break;
    case RSF_ANOMALY_OBSERVER_SCENE:
        rsf_anomaly_geometry_render_scene((unary_fn)original, call->self);
        break;
    default:
        ((unary_fn)original)(call->self);
        break;
    }
}

static bool completes_after(uint32_t boundary)
{
    return boundary == RSF_ANOMALY_OBSERVER_BEGIN || boundary == RSF_ANOMALY_OBSERVER_END ||
        boundary == RSF_ANOMALY_OBSERVER_CAMERA || boundary == RSF_ANOMALY_OBSERVER_TARGET_COPY;
}

static void run(const invocation* call, bool completes)
{
    call_original(call);
    if (completes && completes_after(call->boundary)) {
        rsf_anomaly_observer_after(&platform, call->boundary, call->view, call->projection, call->target);
    }
}

// The engine original may unwind through C++. The invocation is retired on return and during that unwind: by the
// termination handler under MSVC, by the cleanup handler under GCC (built with -fexceptions).
#ifdef _MSC_VER
static void observe(const invocation* call)
{
    const bool completes = rsf_anomaly_observer_before(&platform, call->boundary);
    RSF_FOREIGN_TRY {
        run(call, completes);
    } RSF_FOREIGN_FINALLY {
        rsf_anomaly_observer_leave();
    }
}
#else
static void leave_invocation(const bool* entered) { (void)entered; rsf_anomaly_observer_leave(); }
static void observe(const invocation* call)
{
    const bool completes __attribute__((cleanup(leave_invocation))) =
        rsf_anomaly_observer_before(&platform, call->boundary);
    run(call, completes);
}
#endif

static void render_begin(void* self)
{
    const invocation call = {RSF_ANOMALY_OBSERVER_BEGIN, self, NULL, NULL, 0};
    observe(&call);
}
static void render_end(void* self)
{
    const invocation call = {RSF_ANOMALY_OBSERVER_END, self, NULL, NULL, 0};
    observe(&call);
}
static void camera(void* self, const float* view, const float* projection)
{
    const invocation call = {RSF_ANOMALY_OBSERVER_CAMERA, self, view, projection, 0};
    observe(&call);
}
static void scene(void* self)
{
    const invocation call = {RSF_ANOMALY_OBSERVER_SCENE, self, NULL, NULL, 0};
    observe(&call);
}
static void screen_ui(void* self)
{
    const invocation call = {RSF_ANOMALY_OBSERVER_UI, self, NULL, NULL, 0};
    observe(&call);
}
static void target_copy(void* self, uint32_t target)
{
    const invocation call = {RSF_ANOMALY_OBSERVER_TARGET_COPY, self, NULL, NULL, target};
    observe(&call);
}
static void reset_begin(void* self)
{
    const invocation call = {RSF_ANOMALY_OBSERVER_RESET_BEGIN, self, NULL, NULL, 0};
    observe(&call);
}
static void reset_end(void* self)
{
    const invocation call = {RSF_ANOMALY_OBSERVER_RESET_END, self, NULL, NULL, 0};
    observe(&call);
}

static const char* identify(rsf_anomaly_observer_engine* out)
{
    return rsf_anomaly_observer_identify(out, &rsf_anomaly_guarded_image_headers, guarded_copy);
}

static bool install_hooks(void* image, uint32_t image_size, const rsf_anomaly_hook_site* sites, size_t count)
{
    return rsf_anomaly_hooks_install(&hooks, image, image_size, sites, count);
}

static rsf_anomaly_observer_binding bind(void* replacement, uint32_t boundary)
{
    const rsf_anomaly_observer_binding binding = {replacement, &trampolines[boundary]};
    return binding;
}

static bool enable_hooks(const rsf_anomaly_observer_engine* engine)
{
    if (rsf_anomaly_hooks_created(&hooks)) return rsf_anomaly_hooks_enable(&hooks);
    // MinHook takes code addresses as untyped pointers.
    const rsf_anomaly_observer_binding bindings[observer_site_count] = {
        bind((void*)(uintptr_t)&render_begin, RSF_ANOMALY_OBSERVER_BEGIN),
        bind((void*)(uintptr_t)&render_end, RSF_ANOMALY_OBSERVER_END),
        bind((void*)(uintptr_t)&camera, RSF_ANOMALY_OBSERVER_CAMERA),
        bind((void*)(uintptr_t)&scene, RSF_ANOMALY_OBSERVER_SCENE),
        bind((void*)(uintptr_t)&screen_ui, RSF_ANOMALY_OBSERVER_UI),
        bind((void*)(uintptr_t)&target_copy, RSF_ANOMALY_OBSERVER_TARGET_COPY),
        bind((void*)(uintptr_t)&reset_begin, RSF_ANOMALY_OBSERVER_RESET_BEGIN),
        bind((void*)(uintptr_t)&reset_end, RSF_ANOMALY_OBSERVER_RESET_END),
    };
    return rsf_anomaly_observer_install(engine, bindings, install_hooks);
}

static bool disable_hooks(void) { return rsf_anomaly_hooks_disable(&hooks); }

static bool install_geometry(const rsf_anomaly_observer_engine* engine)
{
    return rsf_anomaly_geometry_install_native(engine->image, engine->build);
}
static bool install_sr(const rsf_anomaly_observer_engine* engine)
{
    return rsf_anomaly_sr_install_native(engine->image, engine->build);
}
static bool install_motion(const rsf_anomaly_observer_engine* engine)
{
    return rsf_anomaly_motion_install_native(engine->image, engine->build);
}
static bool enable_input(void) { return rsf_anomaly_slot_set(RSF_ANOMALY_INPUT_SLOT, true); }
static bool disable_input(void) { return rsf_anomaly_slot_set(RSF_ANOMALY_INPUT_SLOT, false); }

static bool read_frame(const rsf_anomaly_observer_engine* engine, rsf_anomaly_observer_frame* out)
{
    return rsf_anomaly_observer_read_frame(engine, guarded_copy, out);
}


const rsf_anomaly_observer_platform* rsf_anomaly_observer_native(void)
{
    return &platform;
}
