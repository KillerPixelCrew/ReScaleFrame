// SPDX-License-Identifier: MIT
#pragma once
#include "geometry_registry_platform.h"
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// Private adapter ABI between the Rust geometry owner (native_geometry.rs) and the C11 platform leaf
// (native_geometry_platform.c). Engine addresses are opaque to Rust and profiles are opaque to the leaf: the leaf
// names a build by its index. Declarations only; every layout is asserted on both sides.
#ifdef __cplusplus
#define RSF_GEOMETRY_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_GEOMETRY_NOEXCEPT
#endif

enum {
    RSF_ANOMALY_GEOMETRY_UNSET = 255,
    RSF_ANOMALY_GEOMETRY_RIGID = 0, RSF_ANOMALY_GEOMETRY_HARDWARE_SKINNED,
    RSF_ANOMALY_GEOMETRY_SINGLE_BONE, RSF_ANOMALY_GEOMETRY_SOFTWARE_SKINNED,
    RSF_ANOMALY_GEOMETRY_TREE,
    RSF_ANOMALY_GEOMETRY_MAIN = 0, RSF_ANOMALY_GEOMETRY_SCOPE,
    RSF_ANOMALY_GEOMETRY_WORLD = 0, RSF_ANOMALY_GEOMETRY_FIRST_PERSON,
};

typedef bool (*rsf_anomaly_geometry_read_fn)(void*, const void*, size_t) RSF_GEOMETRY_NOEXCEPT;

// The mapped executable and the index of its Rust-owned profiles (native_profile.rs order: Standard, AVX).
typedef struct rsf_anomaly_geometry_engine { uintptr_t module, build; } rsf_anomaly_geometry_engine;
typedef struct rsf_anomaly_geometry_native_matrix { float values[16]; } rsf_anomaly_geometry_native_matrix;
// Row-major, column vectors, widened from the engine's row-vector Fmatrix.
typedef struct rsf_anomaly_geometry_matrix { double values[16]; } rsf_anomaly_geometry_matrix;

typedef struct rsf_anomaly_geometry_context {
    struct rsf_anomaly_geometry_context* previous;
    uint8_t view, domain;
} rsf_anomaly_geometry_context;
typedef struct rsf_anomaly_geometry_window {
    struct rsf_anomaly_geometry_window* previous;
    rsf_anomaly_geometry_context near_context;
    bool near_active;
} rsf_anomaly_geometry_window;
// One visual on the render thread's stack. Rust links the node, the native frame unlinks it, including on unwind.
typedef struct rsf_anomaly_geometry_visual {
    struct rsf_anomaly_geometry_visual* previous;
    const rsf_anomaly_geometry_registry* registry;
    rsf_anomaly_geometry_key key;
    uintptr_t world, skeleton;
    uint64_t visual;
    uint8_t pass, type, verdict, kind, view, domain;
} rsf_anomaly_geometry_visual;
// What the draw-time call site observed, before any capture decision.
typedef struct rsf_anomaly_geometry_facts {
    uint64_t visual;
    int64_t tree_lod;
    uint8_t pass, visual_pass, type, verdict, kind, view, domain;
    bool in_visual, detail;
} rsf_anomaly_geometry_facts;

// The guarded draw copy. `world` is null when unreadable or not affine, `bones` is null without a verified palette,
// and 255 denotes a missing view or domain. It borrows its storage from the capture that produced it.
typedef struct rsf_anomaly_geometry_snapshot {
    rsf_anomaly_geometry_key key;
    const rsf_anomaly_geometry_matrix* world;
    const rsf_anomaly_geometry_matrix* bones;
    size_t bone_count;
    uint64_t palette_layout;
    uint8_t view, domain, draw_kind;
} rsf_anomaly_geometry_snapshot;
// What the flush callback receives: valid only through that synchronous call. `capture` is the identified main-pass
// capture of this draw, else null. `verdict` and `type` are the entry classification, `pass` the phase at the flush.
typedef struct rsf_anomaly_geometry_draw_facts {
    const rsf_anomaly_geometry_snapshot* capture;
    uint64_t visual;
    int64_t tree_lod;
    uint8_t pass, kind, verdict, view, domain, type, visual_pass;
    bool in_visual, detail;
} rsf_anomaly_geometry_draw_facts;

typedef void (*rsf_anomaly_geometry_retire_fn)(void*, const rsf_anomaly_geometry_key*) RSF_GEOMETRY_NOEXCEPT;
typedef void (*rsf_anomaly_geometry_draw_fn)(void*, const rsf_anomaly_geometry_snapshot*) RSF_GEOMETRY_NOEXCEPT;
typedef void (*rsf_anomaly_geometry_flush_fn)(const rsf_anomaly_geometry_draw_facts*) RSF_GEOMETRY_NOEXCEPT;
typedef void (*rsf_anomaly_geometry_near_fn)(void*) RSF_GEOMETRY_NOEXCEPT;

typedef struct rsf_anomaly_geometry_writer {
    // Lends `count` initialized elements until finish. Null declines palette storage.
    rsf_anomaly_geometry_matrix* (*begin)(void*, const rsf_anomaly_geometry_visual*,
        const rsf_anomaly_geometry_matrix*, uint32_t count) RSF_GEOMETRY_NOEXCEPT;
    const rsf_anomaly_geometry_snapshot* (*finish)(void*, uint64_t layout) RSF_GEOMETRY_NOEXCEPT;
} rsf_anomaly_geometry_writer;

// The hook set bound to the production MinHook platform and the two guarded engine-memory leaves.
typedef struct rsf_anomaly_geometry_platform {
    bool (*created)(void) RSF_GEOMETRY_NOEXCEPT;
    bool (*install)(rsf_anomaly_geometry_engine, uint32_t) RSF_GEOMETRY_NOEXCEPT;
    bool (*enable)(void) RSF_GEOMETRY_NOEXCEPT;
    bool (*disable)(void) RSF_GEOMETRY_NOEXCEPT;
    rsf_anomaly_geometry_read_fn read;
    bool (*bones)(uintptr_t first, size_t stride, uint32_t count,
        rsf_anomaly_geometry_native_matrix*) RSF_GEOMETRY_NOEXCEPT;
} rsf_anomaly_geometry_platform;

typedef struct rsf_anomaly_geometry_stats {
    uint64_t models_registered, models_retired, draws_identified, draws_unidentified, capture_faults;
} rsf_anomaly_geometry_stats;
typedef struct rsf_anomaly_geometry_coverage {
    uint64_t draws[RSF_ANOMALY_PASS_COUNT][RSF_ANOMALY_TYPE_COUNT][RSF_ANOMALY_DRAW_COUNT];
    uint64_t flush_total[RSF_ANOMALY_PASS_COUNT], flush_in_visual[RSF_ANOMALY_PASS_COUNT];
    uint64_t captured, capture_stale, capture_no_world, capture_no_palette, capture_no_context, sunk;
    uint64_t detail_renders[RSF_ANOMALY_PASS_COUNT], flush_in_detail[RSF_ANOMALY_PASS_COUNT];
    uint64_t views_main, views_scope, hud_scopes, near_windows;
} rsf_anomaly_geometry_coverage;

// native_geometry.rs
bool rsf_anomaly_geometry_install(const rsf_anomaly_geometry_platform*, rsf_anomaly_geometry_engine, uint32_t);
bool rsf_anomaly_geometry_disable(const rsf_anomaly_geometry_platform*);
bool rsf_anomaly_geometry_active(void);
rsf_anomaly_geometry_engine rsf_anomaly_geometry_binding(void);
void rsf_anomaly_geometry_test_bind(rsf_anomaly_geometry_engine);
void rsf_anomaly_geometry_set_retired(uintptr_t, rsf_anomaly_geometry_retire_fn);
void rsf_anomaly_geometry_set_draw(uintptr_t, rsf_anomaly_geometry_draw_fn);
void rsf_anomaly_geometry_set_flush(rsf_anomaly_geometry_flush_fn);
void rsf_anomaly_geometry_set_near(rsf_anomaly_geometry_near_fn);
void rsf_anomaly_geometry_publish_flush(const rsf_anomaly_geometry_draw_facts*);
void rsf_anomaly_geometry_after_near(void*);
void rsf_anomaly_geometry_set_foliage(bool);
bool rsf_anomaly_geometry_foliage(void);
void rsf_anomaly_geometry_reset(void);
void rsf_anomaly_geometry_factory_enter(void);
void rsf_anomaly_geometry_factory_leave(void);
void rsf_anomaly_geometry_factory_complete(const rsf_anomaly_geometry_platform*, uintptr_t owner, uintptr_t visual);
void rsf_anomaly_geometry_model_delete(const rsf_anomaly_geometry_platform*, uintptr_t owner, uintptr_t reference);
bool rsf_anomaly_geometry_read_world(const rsf_anomaly_geometry_platform*, float* out);
const void* rsf_anomaly_geometry_borrow_draw(void);
const rsf_anomaly_geometry_snapshot* rsf_anomaly_geometry_capture(const rsf_anomaly_geometry_platform*,
    const rsf_anomaly_geometry_writer*, void*);
const rsf_anomaly_geometry_snapshot* rsf_anomaly_geometry_observe(const rsf_anomaly_geometry_platform*,
    const rsf_anomaly_geometry_writer*, void*, rsf_anomaly_geometry_facts*);
// The hook records of `build` pair each profiled entry with the native replacement and the address of its original
// slot. False for an unknown build or a count other than the profiled site count.
struct rsf_anomaly_hook_site;
bool rsf_anomaly_geometry_build_hook_sites(size_t build, void* const* replacements, void** originals,
    struct rsf_anomaly_hook_site* sites, size_t count);
// The profiled SizeOfImage of `build`, or 0 for an unknown build.
uint32_t rsf_anomaly_geometry_image_size(size_t build);
// Guarded access to the staged vertex constants of `slot`; false on any mismatch or fault.
bool rsf_anomaly_geometry_read_vertex(const rsf_anomaly_geometry_platform*, uint32_t slot, uint32_t expected_size,
    uint32_t offset, void* out, uint32_t bytes);
bool rsf_anomaly_geometry_write_vertex(const rsf_anomaly_geometry_platform*, uint32_t slot, uint32_t expected_size,
    uint32_t offset, const void* in, uint32_t bytes);

// scopes.rs
uint8_t rsf_anomaly_geometry_view(const rsf_anomaly_geometry_platform*, rsf_anomaly_geometry_engine);
void rsf_anomaly_geometry_context_enter(rsf_anomaly_geometry_context*, uint8_t view, uint8_t domain, bool inherit);
bool rsf_anomaly_geometry_render_enter(const rsf_anomaly_geometry_platform*, rsf_anomaly_geometry_context*);
bool rsf_anomaly_geometry_hud_enter(rsf_anomaly_geometry_context*);
void rsf_anomaly_geometry_context_leave(rsf_anomaly_geometry_context*);
void rsf_anomaly_geometry_window_enter(rsf_anomaly_geometry_window*);
void rsf_anomaly_geometry_window_leave(rsf_anomaly_geometry_window*);
void rsf_anomaly_geometry_near_leave(void);
void rsf_anomaly_geometry_detail_enter(const rsf_anomaly_geometry_platform*);
void rsf_anomaly_geometry_detail_leave(void);
void rsf_anomaly_geometry_visual_enter(rsf_anomaly_geometry_visual*);
void rsf_anomaly_geometry_visual_source_enter(const rsf_anomaly_geometry_platform*, rsf_anomaly_geometry_visual*,
    uintptr_t visual, uint8_t source);
void rsf_anomaly_geometry_visual_leave(const rsf_anomaly_geometry_visual*);

// native_geometry/coverage.rs
void rsf_anomaly_geometry_stats_read(rsf_anomaly_geometry_stats*);
void rsf_anomaly_geometry_coverage_read(rsf_anomaly_geometry_coverage*);
void rsf_anomaly_geometry_report(void (*)(void*, const char*), void*);
const char* rsf_anomaly_geometry_pass_name(uint8_t);
const char* rsf_anomaly_geometry_type_name(uint8_t);
const char* rsf_anomaly_geometry_verdict_name(uint8_t);

#ifdef __cplusplus
}
#endif
#undef RSF_GEOMETRY_NOEXCEPT
