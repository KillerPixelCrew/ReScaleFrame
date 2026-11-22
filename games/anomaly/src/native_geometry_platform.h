// SPDX-License-Identifier: MIT
#pragma once
#include "geometry_registry_platform.h"
#include "native_profile.h"
#include "../temporal/math.h"

// Private adapter ABI. Engine addresses are opaque to Rust. Only the platform
// leaves access the engine's memory or trampoline tables.
enum {
    RSF_ANOMALY_GEOMETRY_UNSET = 255,
    RSF_ANOMALY_GEOMETRY_RIGID = 0, RSF_ANOMALY_GEOMETRY_HARDWARE_SKINNED,
    RSF_ANOMALY_GEOMETRY_SINGLE_BONE, RSF_ANOMALY_GEOMETRY_SOFTWARE_SKINNED,
    RSF_ANOMALY_GEOMETRY_TREE,
    RSF_ANOMALY_GEOMETRY_MAIN = 0, RSF_ANOMALY_GEOMETRY_SCOPE,
    RSF_ANOMALY_GEOMETRY_WORLD = 0, RSF_ANOMALY_GEOMETRY_FIRST_PERSON,
};

struct rsf_anomaly_geometry_engine { uintptr_t module, profile; };
struct rsf_anomaly_geometry_child_range { uintptr_t first, end, capacity; };
struct rsf_anomaly_geometry_hierarchy {
    uint32_t vtable_rvas[4];
    uint32_t children_offset;
    bool (*read)(void*, const void*, size_t) noexcept;
};
struct rsf_anomaly_geometry_layout { uint32_t phase, viewport, frame, world; };
struct rsf_anomaly_geometry_native_matrix { float values[16]; };
using rsf_anomaly_geometry_matrix = rsf::anomaly::temporal::Mat4;

struct rsf_anomaly_geometry_context {
    rsf_anomaly_geometry_context* previous;
    uint8_t view, domain;
};
struct rsf_anomaly_geometry_window {
    rsf_anomaly_geometry_window* previous;
    rsf_anomaly_geometry_context near_context;
    bool near_active;
};
struct rsf_anomaly_geometry_visual {
    rsf_anomaly_geometry_visual* previous;
    const rsf_anomaly_geometry_registry* registry;
    const void* facade;
    rsf_anomaly_geometry_key key;
    uintptr_t world, skeleton;
    uint64_t visual;
    uint8_t pass, type, verdict, kind, view, domain;
};
struct rsf_anomaly_geometry_facts {
    uint64_t visual;
    int64_t tree_lod;
    uint8_t pass, visual_pass, type, verdict, kind, view, domain;
    bool in_visual, detail;
};

namespace rsf::anomaly::geometry {
struct DrawCapture;
struct DrawTimeFacts;
}
using rsf_anomaly_geometry_retire_fn = void (*)(void*, const rsf_anomaly_geometry_key*) noexcept;
using rsf_anomaly_geometry_draw_fn = void (*)(void*, const rsf::anomaly::geometry::DrawCapture&) noexcept;
using rsf_anomaly_geometry_flush_fn = void (*)(const rsf::anomaly::geometry::DrawTimeFacts&) noexcept;
using rsf_anomaly_geometry_near_fn = void (*)(void*) noexcept;

struct rsf_anomaly_geometry_writer {
    // Lends count initialized elements until finish. Null declines palette storage.
    rsf_anomaly_geometry_matrix* (*begin)(void*, const rsf_anomaly_geometry_visual*,
        const rsf_anomaly_geometry_matrix*, uint32_t count) noexcept;
    const rsf::anomaly::geometry::DrawCapture* (*finish)(void*, uint64_t) noexcept;
};
// The selected build's immutable guard facts. Each pointer borrows a static profile table, and read is the guarded
// engine-memory leaf behind Rust's checked-image reader.
struct rsf_anomaly_geometry_guard_profile {
    bool (*read)(void*, const void*, size_t) noexcept;
    const rsf::anomaly::NativeSite* layout_sites;
    size_t layout_count;
    const rsf::anomaly::NativeSite* sites;
    size_t site_count;
    const rsf::anomaly::UniqueGuard* unique;
    size_t unique_count;
    uint32_t image_size;
};
struct rsf_anomaly_geometry_platform {
    rsf_anomaly_geometry_guard_profile (*guard_profile)(rsf_anomaly_geometry_engine) noexcept;
    bool (*created)() noexcept;
    bool (*install)(rsf_anomaly_geometry_engine, uint32_t) noexcept;
    bool (*enable)() noexcept;
    bool (*disable)() noexcept;
    rsf_anomaly_geometry_hierarchy (*hierarchy)(rsf_anomaly_geometry_engine) noexcept;
    rsf_anomaly_geometry_layout (*layout)(rsf_anomaly_geometry_engine) noexcept;
    bool (*read)(void*, const void*, size_t) noexcept;
    bool (*bones)(uintptr_t first, size_t stride, uint32_t count, rsf_anomaly_geometry_native_matrix*) noexcept;
};

struct rsf_anomaly_geometry_stats {
    uint64_t models_registered, models_retired, draws_identified, draws_unidentified, capture_faults;
};
struct rsf_anomaly_geometry_coverage {
    uint64_t draws[RSF_ANOMALY_PASS_COUNT][RSF_ANOMALY_TYPE_COUNT][RSF_ANOMALY_DRAW_COUNT];
    uint64_t flush_total[RSF_ANOMALY_PASS_COUNT], flush_in_visual[RSF_ANOMALY_PASS_COUNT];
    uint64_t captured, capture_stale, capture_no_world, capture_no_palette, capture_no_context, sunk;
    uint64_t detail_renders[RSF_ANOMALY_PASS_COUNT], flush_in_detail[RSF_ANOMALY_PASS_COUNT];
    uint64_t views_main, views_scope, hud_scopes, near_windows;
};

extern "C" {
bool rsf_anomaly_geometry_install(const rsf_anomaly_geometry_platform*, rsf_anomaly_geometry_engine, uint32_t);
bool rsf_anomaly_geometry_disable(const rsf_anomaly_geometry_platform*);
bool rsf_anomaly_geometry_active();
rsf_anomaly_geometry_engine rsf_anomaly_geometry_binding();
void rsf_anomaly_geometry_test_bind(rsf_anomaly_geometry_engine);
void rsf_anomaly_geometry_set_retired(uintptr_t, rsf_anomaly_geometry_retire_fn);
void rsf_anomaly_geometry_set_draw(uintptr_t, rsf_anomaly_geometry_draw_fn);
void rsf_anomaly_geometry_set_flush(rsf_anomaly_geometry_flush_fn);
void rsf_anomaly_geometry_set_near(rsf_anomaly_geometry_near_fn);
void rsf_anomaly_geometry_publish_flush(const rsf::anomaly::geometry::DrawTimeFacts*);
void rsf_anomaly_geometry_after_near(void*);
void rsf_anomaly_geometry_set_foliage(bool);
bool rsf_anomaly_geometry_foliage();
void rsf_anomaly_geometry_reset();
void rsf_anomaly_geometry_factory_enter();
void rsf_anomaly_geometry_factory_leave();
void rsf_anomaly_geometry_factory_complete(const rsf_anomaly_geometry_platform*, uintptr_t owner, uintptr_t visual);
void rsf_anomaly_geometry_model_delete(const rsf_anomaly_geometry_platform*, uintptr_t owner, uintptr_t reference);
uint8_t rsf_anomaly_geometry_view(const rsf_anomaly_geometry_platform*, rsf_anomaly_geometry_engine);
bool rsf_anomaly_geometry_read_world(const rsf_anomaly_geometry_platform*, float* out);
void rsf_anomaly_geometry_context_enter(rsf_anomaly_geometry_context*, uint8_t view, uint8_t domain, bool inherit);
bool rsf_anomaly_geometry_render_enter(const rsf_anomaly_geometry_platform*, rsf_anomaly_geometry_context*);
bool rsf_anomaly_geometry_hud_enter(rsf_anomaly_geometry_context*);
void rsf_anomaly_geometry_context_leave(rsf_anomaly_geometry_context*);
void rsf_anomaly_geometry_window_enter(rsf_anomaly_geometry_window*);
void rsf_anomaly_geometry_window_leave(rsf_anomaly_geometry_window*);
void rsf_anomaly_geometry_near_leave();
void rsf_anomaly_geometry_detail_enter(const rsf_anomaly_geometry_platform*);
void rsf_anomaly_geometry_detail_leave();
void rsf_anomaly_geometry_visual_enter(rsf_anomaly_geometry_visual*);
void rsf_anomaly_geometry_visual_source_enter(const rsf_anomaly_geometry_platform*, rsf_anomaly_geometry_visual*,
    uintptr_t visual, uint8_t source);
void rsf_anomaly_geometry_visual_leave(const rsf_anomaly_geometry_visual*);
const void* rsf_anomaly_geometry_borrow_draw();
const rsf::anomaly::geometry::DrawCapture* rsf_anomaly_geometry_capture(const rsf_anomaly_geometry_platform*, const rsf_anomaly_geometry_writer*, void*);
const rsf::anomaly::geometry::DrawCapture* rsf_anomaly_geometry_observe(const rsf_anomaly_geometry_platform*, const rsf_anomaly_geometry_writer*, void*,
    rsf_anomaly_geometry_facts*);
void rsf_anomaly_geometry_stats_read(rsf_anomaly_geometry_stats*);
void rsf_anomaly_geometry_coverage_read(rsf_anomaly_geometry_coverage*);
void rsf_anomaly_geometry_report(void (*)(void*, const char*), void*);
const char* rsf_anomaly_geometry_pass_name(uint8_t);
const char* rsf_anomaly_geometry_type_name(uint8_t);
const char* rsf_anomaly_geometry_verdict_name(uint8_t);
}
