/* SPDX-License-Identifier: MIT */
#pragma once
/* Private x64 ABI between the Rust SR owners and the D3D11 leaves. Rust (sr_d3d11.rs and sr_d3d11/native.rs) owns
   twin creation, the passes and the typed COM calls; sr_d3d11_platform.c supplies the four foreign leaves below and
   checks the descriptor layouts against the Windows SDK. */
#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
#define RSF_SR_D3D11_NOEXCEPT noexcept
#define RSF_SR_D3D11_ASSERT(condition, message) static_assert(condition, message)
extern "C" {
#else
#define RSF_SR_D3D11_NOEXCEPT
#define RSF_SR_D3D11_ASSERT(condition, message) _Static_assert(condition, message)
#endif

struct rsf_sr_target;
struct rsf_sr_stage_resources;
struct rsf_sr_temporal_targets;
struct rsf_anomaly_swap_request;
struct rsf_anomaly_swap_twins;
struct rsf_anomaly_swap_fields;
struct rsf_sr_pass;
typedef void (*rsf_sr_log)(void*, const char*);

typedef struct rsf_sr_extent { uint32_t width, height; } rsf_sr_extent;
/* D3D11_TEXTURE2D_DESC, field for field. */
typedef struct rsf_sr_texture {
    rsf_sr_extent size;
    uint32_t mips, layers, format, samples, sample_quality, usage, bindings, cpu_access, misc;
} rsf_sr_texture;
typedef struct rsf_sr_view {
    uint32_t texture2d, format, flags;
    uint32_t descriptor[6];
} rsf_sr_view;
typedef struct rsf_sr_draw {
    void *vertex, *pixel, *constants, *raster, *blend, *depth;
    rsf_sr_extent size;
} rsf_sr_draw;
/* A non-null COM result transfers one reference. The draw callbacks borrow
   the context, bindings and constants for the duration of the call. */
typedef struct rsf_sr_d3d11 {
    void (*retain)(void*);
    void (*release)(void*);
    void* (*as_texture)(void*);
    void* (*view_resource)(void*);
    void* (*texture_device)(void*);
    void (*describe_texture)(void*, rsf_sr_texture*);
    void (*describe_view)(void*, uint32_t, rsf_sr_view*);
    uint32_t (*format_support)(void*, uint32_t);
    void* (*texture)(void*, const rsf_sr_texture*, const void*, uint32_t);
    void* (*view)(void*, void*, uint32_t, const rsf_sr_view*);
    /* Shader compilation returns 1 on success, 0 on refusal, and -1 on exception. */
    int32_t (*shader)(void*, uint32_t, uint32_t, const char*, rsf_sr_log, void*, void**);
    void* (*constant_buffer)(void*);
    void* (*raster_state)(void*);
    void* (*blend_state)(void*);
    void* (*depth_state)(void*, uint32_t);
    void* (*save_state)(void*);
    void (*restore_state)(void*, void*);
    void (*update_constants)(void*, void*, const uint32_t*);
    void (*ps_constant)(void*, uint32_t, void*);
    void (*targets)(void*, uint32_t, void* const*, void*);
    void (*ps_resources)(void*, uint32_t, uint32_t, void* const*);
    void (*clear_stencil)(void*, void*);
    void (*draw)(void*, const rsf_sr_draw*);
} rsf_sr_d3d11;

/* The native leaves of the table. `default_view` creates a default shader view (an owned reference or null).
   `guarded_log` calls a log sink and returns false if it raised a C++ exception, which it contains.
   `save_state` returns an owned snapshot of the immediate-context pipeline or null; `restore_state` restores and
   frees it. */
typedef struct rsf_sr_d3d11_foreign {
    void* (*default_view)(void* device, void* texture);
    bool (*guarded_log)(rsf_sr_log log, void* user, const char* message);
    void* (*save_state)(void* context);
    void (*restore_state)(void* context, void* snapshot);
} rsf_sr_d3d11_foreign;

typedef struct rsf_sr_twin_input {
    void *texture, *rtv, *srv, *uav;
    rsf_sr_extent size;
    uint32_t format, promote, with_uav, world;
} rsf_sr_twin_input;
/* The snapshot pointers borrow from the returned Rust owner. */
typedef struct rsf_sr_target_view {
    void *texture, *rtv, *srv, *uav, *dsv;
    rsf_sr_extent size;
    uint32_t format;
    rsf_sr_texture descriptor;
} rsf_sr_target_view;
typedef struct rsf_sr_temporal_view {
    void *colour, *motion, *output, *exposure, *empty_motion;
    void *colour_rtv, *motion_rtv;
    void *colour_srv, *motion_srv, *output_srv, *empty_motion_srv;
} rsf_sr_temporal_view;
typedef struct rsf_sr_twin_batch_info {
    uint64_t bytes;
    uint32_t made, failed_index;
    uint8_t cause;
    bool pass_refused;
} rsf_sr_twin_batch_info;
typedef struct rsf_sr_draw_views {
    rsf_sr_extent output, source;
    void *targets[3], *depth, *inputs[5];
    uint32_t first_person_depth, first_person_zero;
} rsf_sr_draw_views;

RSF_SR_D3D11_ASSERT(sizeof(rsf_sr_extent) == 8 && sizeof(rsf_sr_texture) == 44 && sizeof(rsf_sr_view) == 36,
    "SR D3D11 descriptor layout");
RSF_SR_D3D11_ASSERT(sizeof(rsf_sr_draw) == 56 && sizeof(rsf_sr_d3d11) == 184 && sizeof(rsf_sr_draw_views) == 96,
    "SR D3D11 table layout");
RSF_SR_D3D11_ASSERT(sizeof(rsf_sr_twin_input) == 56 && sizeof(rsf_sr_target_view) == 96 &&
    sizeof(rsf_sr_temporal_view) == 88, "SR D3D11 target layout");
RSF_SR_D3D11_ASSERT(sizeof(rsf_sr_twin_batch_info) == 24 && sizeof(rsf_sr_d3d11_foreign) == 32,
    "SR D3D11 batch and foreign layout");

/* View kinds and pass kinds (sr_d3d11.rs). */
#define RSF_SR_RTV 0u
#define RSF_SR_SRV 1u
#define RSF_SR_UAV 2u
#define RSF_SR_DSV 3u
#define RSF_SR_DEPTH_SRV 4u
#define RSF_SR_ROUND_TRIP 0u
#define RSF_SR_UPSCALE 1u
#define RSF_SR_PREPARE 2u
#define RSF_SR_REINSERT 3u

/* The process-lifetime table. The first registration's foreign leaves are kept. */
const rsf_sr_d3d11* rsf_anomaly_sr_d3d11_register(const rsf_sr_d3d11_foreign*) RSF_SR_D3D11_NOEXCEPT;
/* The production table with the native leaves of sr_d3d11_platform.c. */
const rsf_sr_d3d11* rsf_anomaly_sr_d3d11_platform(void) RSF_SR_D3D11_NOEXCEPT;
/* The short name of a twin refusal cause (rsf_sr_twin_batch_info::cause). */
const char* rsf_anomaly_sr_twin_error_name(uint8_t) RSF_SR_D3D11_NOEXCEPT;

/* Typed Rust leaves used by the table. */
void rsf_anomaly_sr_retain(void*) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_release(void*) RSF_SR_D3D11_NOEXCEPT;
void* rsf_anomaly_sr_as_texture(void*) RSF_SR_D3D11_NOEXCEPT;
void* rsf_anomaly_sr_view_resource(void*) RSF_SR_D3D11_NOEXCEPT;
void* rsf_anomaly_sr_texture_device(void*) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_describe_texture(void*, rsf_sr_texture*) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_describe_view(void*, uint32_t, rsf_sr_view*) RSF_SR_D3D11_NOEXCEPT;
uint32_t rsf_anomaly_sr_format_support(void*, uint32_t) RSF_SR_D3D11_NOEXCEPT;
void* rsf_anomaly_sr_texture(void*, const rsf_sr_texture*, const void*, uint32_t) RSF_SR_D3D11_NOEXCEPT;
void* rsf_anomaly_sr_view(void*, void*, uint32_t, const rsf_sr_view*) RSF_SR_D3D11_NOEXCEPT;
int32_t rsf_anomaly_sr_compile_shader(void*, uint32_t, uint32_t, const char*,
    bool (*)(void*, const char*), void*, void**) RSF_SR_D3D11_NOEXCEPT;
void* rsf_anomaly_sr_constant_buffer(void*) RSF_SR_D3D11_NOEXCEPT;
void* rsf_anomaly_sr_raster_state(void*) RSF_SR_D3D11_NOEXCEPT;
void* rsf_anomaly_sr_blend_state(void*) RSF_SR_D3D11_NOEXCEPT;
void* rsf_anomaly_sr_depth_state(void*, uint32_t) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_update_constants(void*, void*, const uint32_t*) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_ps_constant(void*, uint32_t, void*) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_targets(void*, uint32_t, void* const*, void*) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_ps_resources(void*, uint32_t, uint32_t, void* const*) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_clear_stencil(void*, void*) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_draw(void*, const rsf_sr_draw*) RSF_SR_D3D11_NOEXCEPT;
/* What CRender::rmNormal does for a bound target of this size (r4.cpp:852-858). */
void rsf_anomaly_sr_set_full_viewport(void*, uint32_t, uint32_t) RSF_SR_D3D11_NOEXCEPT;

/* Stage query and readback leaves. Capture owns the map lifetime. */
void* rsf_anomaly_sr_timer_retain_device(void*) RSF_SR_D3D11_NOEXCEPT;
void* rsf_anomaly_sr_timer_create_query(void*, bool) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_timer_issue(void*, void*, bool) RSF_SR_D3D11_NOEXCEPT;
int32_t rsf_anomaly_sr_timer_clock(void*, void*, uint64_t*, bool*) RSF_SR_D3D11_NOEXCEPT;
int32_t rsf_anomaly_sr_timer_timestamp(void*, void*, uint64_t*) RSF_SR_D3D11_NOEXCEPT;
bool rsf_anomaly_sr_immediate_context(void*) RSF_SR_D3D11_NOEXCEPT;
void* rsf_anomaly_sr_context_device(void*) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_copy_subresource(void*, void*, uint32_t, void*, uint32_t) RSF_SR_D3D11_NOEXCEPT;
bool rsf_anomaly_sr_map_read(void*, void*, uint32_t, const void**, uint32_t*) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_unmap(void*, void*, uint32_t) RSF_SR_D3D11_NOEXCEPT;
uint32_t rsf_anomaly_sr_twin_refs(void*) RSF_SR_D3D11_NOEXCEPT;

/* The swap host serializes access. Pass and depth pointers borrow the current resources. */
uint32_t rsf_anomaly_sr_stage_make_twins(struct rsf_sr_stage_resources**, const rsf_sr_d3d11*, void*,
    const struct rsf_anomaly_swap_request*, struct rsf_anomaly_swap_twins*, rsf_sr_twin_batch_info*, rsf_sr_log,
    void*) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_stage_retire_twins(struct rsf_sr_stage_resources*, uint32_t (*refs)(void*)) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_stage_reset_pass(struct rsf_sr_stage_resources*) RSF_SR_D3D11_NOEXCEPT;
const struct rsf_sr_pass* rsf_anomaly_sr_stage_pass(const struct rsf_sr_stage_resources*) RSF_SR_D3D11_NOEXCEPT;
void* rsf_anomaly_sr_stage_depth(const struct rsf_sr_stage_resources*) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_stage_resource_stats(const struct rsf_sr_stage_resources*, uint64_t*, uint64_t*)
    RSF_SR_D3D11_NOEXCEPT;
uint32_t rsf_anomaly_sr_temporal_ready(const struct rsf_sr_pass*) RSF_SR_D3D11_NOEXCEPT;
uint32_t rsf_anomaly_sr_run_pass(const struct rsf_sr_pass*, void*, uint32_t, const rsf_sr_draw_views*)
    RSF_SR_D3D11_NOEXCEPT;

#ifdef __cplusplus
}
#endif
#undef RSF_SR_D3D11_ASSERT
#undef RSF_SR_D3D11_NOEXCEPT
