// SPDX-License-Identifier: MIT
// Native observer detours, typed trampoline calls and fault boundaries for the Rust observer (native_observer.rs).
#include "native_observer.h"
#include "native_geometry.h"
#include "native_memory.h"

// native_observer.rs.
bool rsf_anomaly_observer_before(uint32_t boundary);
void rsf_anomaly_observer_after(uint32_t boundary,
    const float* view, const float* projection, uint32_t target);
void rsf_anomaly_observer_leave(void);

// Same order as native_sites in both exact-build records (native_profile.rs).
enum { observer_site_count = RSF_ANOMALY_OBSERVER_RESET_END + 1 };
_Static_assert(observer_site_count == 8, "observer site count");

typedef void (*unary_fn)(void*);
typedef void (*camera_fn)(void*, const float*, const float*);
typedef void (*copy_target_fn)(void*, uint32_t);

// native_observer.rs installs the detours below; MinHook writes each trampoline to this untyped storage. Cast only at
// the native call site.
void* rsf_anomaly_observer_trampolines[observer_site_count];
#define trampolines rsf_anomaly_observer_trampolines

// One detour invocation. Rust admits it before the original and records completion facts after it. Begin, End,
// camera and target-copy facts follow their original calls; the other boundaries report before them.
typedef struct invocation {
    uint32_t boundary;
    void* self;
    const float* view;
    const float* projection;
    uint32_t target;
} invocation;

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
        rsf_anomaly_observer_after(call->boundary, call->view, call->projection, call->target);
    }
}

// The engine original may unwind through C++. The invocation is retired on return and during that unwind: by the
// termination handler under MSVC, by the cleanup handler under GCC (built with -fexceptions).
#ifdef _MSC_VER
static void observe(const invocation* call)
{
    const bool completes = rsf_anomaly_observer_before(call->boundary);
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
        rsf_anomaly_observer_before(call->boundary);
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

// In RSF_ANOMALY_OBSERVER_* order. MinHook takes code addresses as untyped pointers.
void* const rsf_anomaly_observer_detours[observer_site_count] = {
    (void*)&render_begin, (void*)&render_end, (void*)&camera, (void*)&scene,
    (void*)&screen_ui, (void*)&target_copy, (void*)&reset_begin, (void*)&reset_end,
};