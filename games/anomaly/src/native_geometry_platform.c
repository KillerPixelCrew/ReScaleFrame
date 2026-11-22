/* SPDX-License-Identifier: MIT */
/* The geometry producer's native leaf under the Rust owner (native_geometry.rs): the 17 detours with their stack scope
 * guards, the guarded engine reads and the hook set bound to the production MinHook platform. Profiles, admission,
 * classification and capture decisions stay in Rust; this file only brackets the engine's calls. */
#include "native_geometry.h"
#include "native_memory.h"
#include <stdlib.h>
#include <string.h>

#ifdef _MSC_VER
/* MSVC unwinds through a detour when the engine raises. Each scope must still be left. */
#define RSF_TRY RSF_FOREIGN_TRY {
#define RSF_FINALLY } RSF_FOREIGN_FINALLY {
#define RSF_END_TRY }
#else
#define RSF_TRY {
#define RSF_FINALLY } {
#define RSF_END_TRY }
#endif

#define SITE_COUNT 17u
enum {
    SITE_CREATE, SITE_CHILD, SITE_DUPLICATE, SITE_DELETE, SITE_STATIC, SITE_PROGRESSIVE, SITE_SKELETON_ST,
    SITE_SKELETON_PM, SITE_TREE_ST, SITE_TREE_PM, SITE_FLUSH, SITE_RENDER_HUD, SITE_RENDER_SORTED,
    SITE_RENDER_EMISSIVE, SITE_RM_NEAR, SITE_RM_NORMAL, SITE_DETAIL_RENDER,
};

_Static_assert(sizeof(rsf_anomaly_geometry_engine) == 16 && sizeof(rsf_anomaly_geometry_context) == 16 &&
    sizeof(rsf_anomaly_geometry_window) == 32 && sizeof(rsf_anomaly_geometry_visual) == 72, "scope layouts");
_Static_assert(sizeof(rsf_anomaly_geometry_facts) == 32 && sizeof(rsf_anomaly_geometry_native_matrix) == 64 &&
    sizeof(rsf_anomaly_geometry_matrix) == 128 && _Alignof(rsf_anomaly_geometry_matrix) == _Alignof(double) &&
    offsetof(rsf_anomaly_geometry_matrix, values) == 0, "fact and matrix layouts");
_Static_assert(sizeof(rsf_anomaly_geometry_snapshot) == 64 && offsetof(rsf_anomaly_geometry_snapshot, world) == 24 &&
    offsetof(rsf_anomaly_geometry_snapshot, view) == 56 && offsetof(rsf_anomaly_geometry_snapshot, draw_kind) == 58,
    "snapshot layout read by the motion owner");
_Static_assert(sizeof(rsf_anomaly_geometry_writer) == 16 && sizeof(rsf_anomaly_geometry_platform) == 48 &&
    offsetof(rsf_anomaly_geometry_platform, bones) == 40, "writer and platform layouts");
_Static_assert(sizeof(rsf_anomaly_geometry_stats) == 40 && sizeof(rsf_anomaly_geometry_coverage) == 1472,
    "counter layouts");
_Static_assert(sizeof(rsf_anomaly_hook_site) == 48 && offsetof(rsf_anomaly_hook_site, expected) == 12 &&
    offsetof(rsf_anomaly_hook_site, replacement) == 32 && offsetof(rsf_anomaly_hook_site, original) == 40,
    "hook site layout");

/* The Microsoft x64 member ABI puts the visual's second argument in XMM1, which a plain (void*, float) function shares. */
typedef void* (*create_fn)(void*, const char*, void*);
typedef void* (*duplicate_fn)(void*, void*);
typedef void (*delete_fn)(void*, void**, int32_t);
typedef void (*render_fn)(void*, float);
typedef void (*flush_fn)(void);
typedef void (*unary_fn)(void*);

static void* originals[SITE_COUNT];
static rsf_anomaly_hook_set native_hooks;
static rsf_anomaly_hook_site sites[SITE_COUNT];

static bool guarded_read(void* out, const void* address, size_t size) { return guarded_copy(out, address, size); }

static bool bones(uintptr_t first, size_t stride, uint32_t count, rsf_anomaly_geometry_native_matrix* out)
{
    for (uint32_t i = 0; i < count; ++i) {
        const uintptr_t address = first + (size_t)i * stride;
        if (!guarded_copy(&out[i], (const void*)address, sizeof(*out))) return false;
    }
    return true;
}

static void* create_hook(void*, const char*, void*);
static void* child_hook(void*, const char*, void*);
static void* duplicate_hook(void*, void*);
static void delete_hook(void*, void**, int32_t);
static void rigid_hook(void*, float);
static void progressive_hook(void*, float);
static void skeleton_st_hook(void*, float);
static void skeleton_pm_hook(void*, float);
static void tree_st_hook(void*, float);
static void tree_pm_hook(void*, float);
static void flush_hook(void);
static void render_hud_hook(void*);
static void render_sorted_hook(void*);
static void render_emissive_hook(void*);
static void rm_near_hook(void*);
static void rm_normal_hook(void*);
static void detail_render_hook(void*);

static bool fill_sites(size_t build)
{
    void* const replacements[SITE_COUNT] = {
        (void*)(uintptr_t)create_hook, (void*)(uintptr_t)child_hook, (void*)(uintptr_t)duplicate_hook,
        (void*)(uintptr_t)delete_hook, (void*)(uintptr_t)rigid_hook, (void*)(uintptr_t)progressive_hook,
        (void*)(uintptr_t)skeleton_st_hook, (void*)(uintptr_t)skeleton_pm_hook, (void*)(uintptr_t)tree_st_hook,
        (void*)(uintptr_t)tree_pm_hook, (void*)(uintptr_t)flush_hook, (void*)(uintptr_t)render_hud_hook,
        (void*)(uintptr_t)render_sorted_hook, (void*)(uintptr_t)render_emissive_hook,
        (void*)(uintptr_t)rm_near_hook, (void*)(uintptr_t)rm_normal_hook, (void*)(uintptr_t)detail_render_hook,
    };
    return rsf_anomaly_geometry_build_hook_sites(build, replacements, originals, sites, SITE_COUNT);
}

static bool hooks_created(void) { return rsf_anomaly_hooks_created(&native_hooks); }
static bool hooks_enable(void) { return rsf_anomaly_hooks_enable(&native_hooks); }
static bool hooks_disable(void) { return rsf_anomaly_hooks_disable(&native_hooks); }

/* Rust guards the image size, every layout site and every detour entry with its unique prefix before this runs. */
static bool hooks_install(rsf_anomaly_geometry_engine engine, uint32_t size)
{
    return fill_sites(engine.build) &&
        rsf_anomaly_hooks_install(&native_hooks, (void*)engine.module, size, sites, SITE_COUNT);
}

static const rsf_anomaly_geometry_platform platform = {
    hooks_created, hooks_install, hooks_enable, hooks_disable, guarded_read, bones,
};

/* The draw copy. Rust decides when to sample, which rows are valid and when the copy is ready to publish. */
void rsf_anomaly_geometry_capture_release(rsf_anomaly_geometry_capture_storage* storage)
{
    free(storage->bones);
    memset(storage, 0, sizeof(*storage));
}

static rsf_anomaly_geometry_matrix* capture_begin(void* user, const rsf_anomaly_geometry_visual* scope,
    const rsf_anomaly_geometry_matrix* matrix, uint32_t count)
{
    rsf_anomaly_geometry_capture_storage* capture = user;
    rsf_anomaly_geometry_capture_release(capture);
    capture->present = true;
    capture->snapshot.key = scope->key;
    capture->snapshot.draw_kind = scope->kind;
    capture->snapshot.view = scope->view;
    capture->snapshot.domain = scope->domain;
    if (matrix) {
        capture->world = *matrix;
        capture->snapshot.world = &capture->world;
    }
    if (count) {
        capture->bones = calloc(count, sizeof(*capture->bones));
        if (!capture->bones) return NULL;
        capture->snapshot.bones = capture->bones;
        capture->snapshot.bone_count = count;
    }
    return capture->bones;
}

static const rsf_anomaly_geometry_snapshot* capture_finish(void* user, uint64_t layout)
{
    rsf_anomaly_geometry_capture_storage* capture = user;
    if (!layout) {
        free(capture->bones);
        capture->bones = NULL;
        capture->snapshot.bones = NULL;
        capture->snapshot.bone_count = 0;
    }
    capture->snapshot.palette_layout = layout;
    return &capture->snapshot;
}

static const rsf_anomaly_geometry_writer writer = {capture_begin, capture_finish};

const rsf_anomaly_geometry_snapshot* rsf_anomaly_geometry_capture_current(
    rsf_anomaly_geometry_capture_storage* storage)
{
    memset(storage, 0, sizeof(*storage));
    return rsf_anomaly_geometry_capture(&platform, &writer, storage);
}

/* Model lifecycle. The original runs under a factory scope that Rust leaves on success and this frame leaves on unwind. */
static void* create_common(size_t slot, void* self, const char* name, void* reader)
{
    void* result = NULL;
    bool returned = false;
    rsf_anomaly_geometry_factory_enter();
    RSF_TRY
        result = ((create_fn)originals[slot])(self, name, reader);
        returned = true;
    RSF_FINALLY
        if (!returned) rsf_anomaly_geometry_factory_leave();
    RSF_END_TRY
    rsf_anomaly_geometry_factory_complete(&platform, (uintptr_t)self, (uintptr_t)result);
    return result;
}

static void* create_hook(void* self, const char* name, void* reader)
{
    return create_common(SITE_CREATE, self, name, reader);
}

static void* child_hook(void* self, const char* name, void* reader)
{
    return create_common(SITE_CHILD, self, name, reader);
}

static void* duplicate_hook(void* self, void* source)
{
    void* result = NULL;
    bool returned = false;
    rsf_anomaly_geometry_factory_enter();
    RSF_TRY
        result = ((duplicate_fn)originals[SITE_DUPLICATE])(self, source);
        returned = true;
    RSF_FINALLY
        if (!returned) rsf_anomaly_geometry_factory_leave();
    RSF_END_TRY
    rsf_anomaly_geometry_factory_complete(&platform, (uintptr_t)self, (uintptr_t)result);
    return result;
}

static void delete_hook(void* self, void** reference, int32_t discard)
{
    rsf_anomaly_geometry_model_delete(&platform, (uintptr_t)self, (uintptr_t)reference);
    ((delete_fn)originals[SITE_DELETE])(self, reference, discard);
}

/* Visual entry. Rust classifies the draw and links the pinned node before the original runs. */
static void render_visual(size_t slot, void* self, float lod, uint8_t type)
{
    rsf_anomaly_geometry_visual scope;
    memset(&scope, 0, sizeof(scope));
    rsf_anomaly_geometry_visual_source_enter(&platform, &scope, (uintptr_t)self, type);
    RSF_TRY
        ((render_fn)originals[slot])(self, lod);
    RSF_FINALLY
        rsf_anomaly_geometry_visual_leave(&scope);
    RSF_END_TRY
}

static void rigid_hook(void* self, float lod) { render_visual(SITE_STATIC, self, lod, RSF_ANOMALY_TYPE_STATIC); }
static void progressive_hook(void* self, float lod)
{
    render_visual(SITE_PROGRESSIVE, self, lod, RSF_ANOMALY_TYPE_PROGRESSIVE);
}
static void skeleton_st_hook(void* self, float lod)
{
    render_visual(SITE_SKELETON_ST, self, lod, RSF_ANOMALY_TYPE_SKELETON_ST);
}
static void skeleton_pm_hook(void* self, float lod)
{
    render_visual(SITE_SKELETON_PM, self, lod, RSF_ANOMALY_TYPE_SKELETON_PM);
}
static void tree_st_hook(void* self, float lod) { render_visual(SITE_TREE_ST, self, lod, RSF_ANOMALY_TYPE_TREE_ST); }
static void tree_pm_hook(void* self, float lod) { render_visual(SITE_TREE_PM, self, lod, RSF_ANOMALY_TYPE_TREE_PM); }

/* CBackend::constants.flush: capture before the original, publish after the native uploads and before the D3D draw. */
static void flush_hook(void)
{
    rsf_anomaly_geometry_capture_storage capture;
    rsf_anomaly_geometry_facts raw;
    rsf_anomaly_geometry_draw_facts facts;
    memset(&capture, 0, sizeof(capture));
    RSF_TRY
        const rsf_anomaly_geometry_snapshot* kept = rsf_anomaly_geometry_observe(&platform, &writer, &capture, &raw);
        facts.capture = kept;
        facts.visual = raw.visual;
        facts.tree_lod = raw.tree_lod;
        facts.pass = raw.pass;
        facts.kind = raw.kind;
        facts.verdict = raw.verdict;
        facts.view = raw.view;
        facts.domain = raw.domain;
        facts.type = raw.type;
        facts.visual_pass = raw.visual_pass;
        facts.in_visual = raw.in_visual;
        facts.detail = raw.detail;
        ((flush_fn)originals[SITE_FLUSH])();
        rsf_anomaly_geometry_publish_flush(&facts);
    RSF_FINALLY
        rsf_anomaly_geometry_capture_release(&capture);
    RSF_END_TRY
}

/* Context boundaries. */
static void render_hud_hook(void* self)
{
    rsf_anomaly_geometry_context scope;
    memset(&scope, 0, sizeof(scope));
    const bool entered = rsf_anomaly_geometry_hud_enter(&scope);
    RSF_TRY
        ((unary_fn)originals[SITE_RENDER_HUD])(self);
    RSF_FINALLY
        if (entered) rsf_anomaly_geometry_context_leave(&scope);
    RSF_END_TRY
}

static void windowed(size_t slot, void* self)
{
    rsf_anomaly_geometry_window window;
    memset(&window, 0, sizeof(window));
    rsf_anomaly_geometry_window_enter(&window);
    RSF_TRY
        ((unary_fn)originals[slot])(self);
    RSF_FINALLY
        rsf_anomaly_geometry_window_leave(&window);
    RSF_END_TRY
}

static void render_sorted_hook(void* self) { windowed(SITE_RENDER_SORTED, self); }
static void render_emissive_hook(void* self) { windowed(SITE_RENDER_EMISSIVE, self); }

static void rm_near_hook(void* self)
{
    ((unary_fn)originals[SITE_RM_NEAR])(self);
    rsf_anomaly_geometry_after_near(self);
}

static void rm_normal_hook(void* self)
{
    rsf_anomaly_geometry_near_leave();
    ((unary_fn)originals[SITE_RM_NORMAL])(self);
}

static void detail_render_hook(void* self)
{
    rsf_anomaly_geometry_detail_enter(&platform);
    RSF_TRY
        ((unary_fn)originals[SITE_DETAIL_RENDER])(self);
    RSF_FINALLY
        rsf_anomaly_geometry_detail_leave();
    RSF_END_TRY
}

/* Entry points. */
bool rsf_anomaly_geometry_install_native(void* image, size_t build)
{
    const rsf_anomaly_geometry_engine engine = {(uintptr_t)image, build};
    const uint32_t size = rsf_anomaly_geometry_image_size(build);
    return size && rsf_anomaly_geometry_install(&platform, engine, size);
}

bool rsf_anomaly_geometry_disable_native(void) { return rsf_anomaly_geometry_disable(&platform); }

void rsf_anomaly_geometry_render_scene(void (*original)(void*), void* self)
{
    rsf_anomaly_geometry_context scope;
    memset(&scope, 0, sizeof(scope));
    const bool entered = rsf_anomaly_geometry_render_enter(&platform, &scope);
    RSF_TRY
        original(self);
    RSF_FINALLY
        if (entered) rsf_anomaly_geometry_context_leave(&scope);
    RSF_END_TRY
}

bool rsf_anomaly_geometry_render_view_enter(rsf_anomaly_geometry_context* scope)
{
    return rsf_anomaly_geometry_render_enter(&platform, scope);
}

uint8_t rsf_anomaly_geometry_current_view(const void* module, size_t build)
{
    const rsf_anomaly_geometry_engine engine = {(uintptr_t)module, build};
    return rsf_anomaly_geometry_view(&platform, engine);
}

bool rsf_anomaly_geometry_world_matrix(float out[16]) { return rsf_anomaly_geometry_read_world(&platform, out); }

bool rsf_anomaly_geometry_read_constants(uint32_t slot, uint32_t expected_size, uint32_t offset, void* out,
    uint32_t bytes)
{
    return rsf_anomaly_geometry_read_vertex(&platform, slot, expected_size, offset, out, bytes);
}

bool rsf_anomaly_geometry_write_constants(uint32_t slot, uint32_t expected_size, uint32_t offset, const void* in,
    uint32_t bytes)
{
    return rsf_anomaly_geometry_write_vertex(&platform, slot, expected_size, offset, in, bytes);
}

const rsf_anomaly_hook_site* rsf_anomaly_geometry_hook_sites(size_t build)
{
    return fill_sites(build) ? sites : NULL;
}

#ifdef RSF_ANOMALY_GEOMETRY_TESTING
bool rsf_test_anomaly_geometry_guard_lifecycle(void*, const rsf_anomaly_geometry_platform*,
    rsf_anomaly_geometry_engine, rsf_anomaly_geometry_engine);

bool rsf_anomaly_geometry_test_guard_lifecycle(void* module, size_t build, size_t other)
{
    const rsf_anomaly_geometry_engine engine = {(uintptr_t)module, build};
    const rsf_anomaly_geometry_engine foreign = {(uintptr_t)module, other};
    return rsf_test_anomaly_geometry_guard_lifecycle(module, &platform, engine, foreign);
}

void rsf_anomaly_geometry_test_bind_module(void* module, size_t build)
{
    const rsf_anomaly_geometry_engine engine = {(uintptr_t)module, build};
    rsf_anomaly_geometry_test_bind(engine);
}
#endif
