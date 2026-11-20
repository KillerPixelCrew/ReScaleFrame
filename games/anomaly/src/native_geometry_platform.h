// SPDX-License-Identifier: MIT
#pragma once
#include "geometry_registry_platform.h"

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
struct rsf_anomaly_geometry_view_sample { uint32_t frame; uint8_t active, delay; };
struct rsf_anomaly_geometry_palette { uintptr_t instances; uint64_t layout; uint32_t count; uint16_t mode; };
struct rsf_anomaly_geometry_matrix { double values[16]; };

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

using rsf_anomaly_geometry_fn = void (*)();
struct rsf_anomaly_geometry_callback {
    rsf_anomaly_geometry_fn function;
    uintptr_t user;
    void (*invoke)(rsf_anomaly_geometry_fn, uintptr_t, const void*) noexcept;
};
struct rsf_anomaly_geometry_writer {
    bool (*begin)(void*, const rsf_anomaly_geometry_visual*, const rsf_anomaly_geometry_matrix*, uint32_t) noexcept;
    void (*bone)(void*, uint32_t, const rsf_anomaly_geometry_matrix*) noexcept;
    const void* (*finish)(void*, uint64_t) noexcept;
};
struct rsf_anomaly_geometry_platform {
    bool (*guard)(rsf_anomaly_geometry_engine, uint32_t) noexcept;
    bool (*created)() noexcept;
    bool (*install)(rsf_anomaly_geometry_engine, uint32_t) noexcept;
    bool (*enable)() noexcept;
    bool (*disable)() noexcept;
    bool (*children)(rsf_anomaly_geometry_engine, uintptr_t, rsf_anomaly_geometry_child_range*) noexcept;
    bool (*pointer)(uintptr_t, uintptr_t*) noexcept;
    uint8_t (*pass)(rsf_anomaly_geometry_engine) noexcept;
    bool (*view)(rsf_anomaly_geometry_engine, rsf_anomaly_geometry_view_sample*) noexcept;
    uintptr_t (*world)(rsf_anomaly_geometry_engine) noexcept;
    bool (*matrix)(uintptr_t, float*) noexcept;
    bool (*skin_mode)(uintptr_t, uint16_t*) noexcept;
    bool (*palette)(uintptr_t, rsf_anomaly_geometry_palette*) noexcept;
    uintptr_t (*bone_instances)(uintptr_t) noexcept;
    bool (*bone)(uintptr_t, uint32_t, float*) noexcept;
    int64_t (*tree_lod)(uintptr_t) noexcept;
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
void rsf_anomaly_geometry_set_retired(rsf_anomaly_geometry_callback);
void rsf_anomaly_geometry_set_draw(rsf_anomaly_geometry_callback);
void rsf_anomaly_geometry_set_flush(rsf_anomaly_geometry_callback);
void rsf_anomaly_geometry_set_near(rsf_anomaly_geometry_callback);
void rsf_anomaly_geometry_publish_flush(const void*);
void rsf_anomaly_geometry_publish_near(void*);
void rsf_anomaly_geometry_set_foliage(bool);
bool rsf_anomaly_geometry_foliage();
void rsf_anomaly_geometry_reset();
void rsf_anomaly_geometry_factory_enter();
void rsf_anomaly_geometry_factory_leave();
void rsf_anomaly_geometry_model_return(const rsf_anomaly_geometry_platform*, uintptr_t owner, uintptr_t visual);
void rsf_anomaly_geometry_model_delete(const rsf_anomaly_geometry_platform*, uintptr_t owner, uintptr_t reference);
uint8_t rsf_anomaly_geometry_view(const rsf_anomaly_geometry_platform*, rsf_anomaly_geometry_engine);
void rsf_anomaly_geometry_context_enter(rsf_anomaly_geometry_context*, uint8_t view, uint8_t domain, bool inherit);
bool rsf_anomaly_geometry_render_enter(const rsf_anomaly_geometry_platform*, rsf_anomaly_geometry_context*);
bool rsf_anomaly_geometry_hud_enter(rsf_anomaly_geometry_context*);
void rsf_anomaly_geometry_context_leave(rsf_anomaly_geometry_context*);
void rsf_anomaly_geometry_window_enter(rsf_anomaly_geometry_window*);
void rsf_anomaly_geometry_window_leave(rsf_anomaly_geometry_window*);
void rsf_anomaly_geometry_near_enter();
void rsf_anomaly_geometry_near_leave();
void rsf_anomaly_geometry_detail_enter(const rsf_anomaly_geometry_platform*);
void rsf_anomaly_geometry_detail_leave();
void rsf_anomaly_geometry_visual_enter(rsf_anomaly_geometry_visual*);
void rsf_anomaly_geometry_classify_visual(const rsf_anomaly_geometry_platform*, rsf_anomaly_geometry_visual*);
void rsf_anomaly_geometry_visual_leave(const rsf_anomaly_geometry_visual*);
const void* rsf_anomaly_geometry_borrow_draw();
const void* rsf_anomaly_geometry_capture(const rsf_anomaly_geometry_platform*, const rsf_anomaly_geometry_writer*, void*);
const void* rsf_anomaly_geometry_observe(const rsf_anomaly_geometry_platform*, const rsf_anomaly_geometry_writer*, void*,
    rsf_anomaly_geometry_facts*);
void rsf_anomaly_geometry_stats_read(rsf_anomaly_geometry_stats*);
void rsf_anomaly_geometry_coverage_read(rsf_anomaly_geometry_coverage*);
void rsf_anomaly_geometry_report(void (*)(void*, const char*), void*);
const char* rsf_anomaly_geometry_pass_name(uint8_t);
const char* rsf_anomaly_geometry_type_name(uint8_t);
const char* rsf_anomaly_geometry_verdict_name(uint8_t);
}
