// SPDX-License-Identifier: MIT
#pragma once
#include <cstdint>

// Private x64 ABI between the Rust SR owners and the D3D11 leaves.
struct rsf_sr_target;
struct rsf_sr_stage_resources;
struct rsf_sr_temporal_targets;
struct rsf_anomaly_swap_request;
struct rsf_anomaly_swap_twins;
struct rsf_anomaly_swap_fields;
struct rsf_sr_pass;
using rsf_sr_log = void (*)(void*, const char*);

struct rsf_sr_extent { uint32_t width, height; };
struct rsf_sr_texture {
    rsf_sr_extent size;
    uint32_t mips, layers, format, samples, sample_quality, usage, bindings, cpu_access, misc;
};
struct rsf_sr_view {
    uint32_t texture2d, format, flags;
    uint32_t descriptor[6];
};
struct rsf_sr_draw {
    void *vertex, *pixel, *constants, *raster, *blend, *depth;
    rsf_sr_extent size;
};
// A non-null COM result transfers one reference. The draw callbacks borrow
// the context, bindings and constants for the duration of the call.
struct rsf_sr_d3d11 {
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
    // Shader compilation returns 1 on success, 0 on refusal, and -1 on exception.
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
};

struct rsf_sr_twin_input {
    void *texture, *rtv, *srv, *uav;
    rsf_sr_extent size;
    uint32_t format, promote, with_uav, world;
};
// The snapshot pointers borrow from the returned Rust owner.
struct rsf_sr_target_view {
    void *texture, *rtv, *srv, *uav, *dsv;
    rsf_sr_extent size;
    uint32_t format;
    rsf_sr_texture descriptor;
};
struct rsf_sr_temporal_view {
    void *colour, *motion, *output, *exposure, *empty_motion;
    void *colour_rtv, *motion_rtv;
    void *colour_srv, *motion_srv, *output_srv, *empty_motion_srv;
};
struct rsf_sr_twin_batch_info {
    uint64_t bytes;
    uint32_t made, failed_index;
    uint8_t cause;
    bool pass_refused;
};
struct rsf_sr_draw_views {
    rsf_sr_extent output, source;
    void *targets[3], *depth, *inputs[5];
    uint32_t first_person_depth, first_person_zero;
};

static_assert(sizeof(rsf_sr_extent) == 8 && sizeof(rsf_sr_texture) == 44 && sizeof(rsf_sr_view) == 36);
static_assert(sizeof(rsf_sr_draw) == 56 && sizeof(rsf_sr_d3d11) == 184 && sizeof(rsf_sr_draw_views) == 96);
static_assert(sizeof(rsf_sr_twin_input) == 56 && sizeof(rsf_sr_target_view) == 96 && sizeof(rsf_sr_temporal_view) == 88);
static_assert(sizeof(rsf_sr_twin_batch_info) == 24);

enum : uint32_t {
    RSF_SR_RTV, RSF_SR_SRV, RSF_SR_UAV, RSF_SR_DSV, RSF_SR_DEPTH_SRV,
};
enum : uint32_t { RSF_SR_ROUND_TRIP, RSF_SR_UPSCALE, RSF_SR_PREPARE, RSF_SR_REINSERT };

extern "C" {
// Typed Rust leaves used by the native table. The native shader and context-state
// adapters continue to supply their existing failure and allocation contracts.
void rsf_anomaly_sr_retain(void*) noexcept;
void rsf_anomaly_sr_release(void*) noexcept;
void* rsf_anomaly_sr_as_texture(void*) noexcept;
void* rsf_anomaly_sr_view_resource(void*) noexcept;
void* rsf_anomaly_sr_texture_device(void*) noexcept;
void rsf_anomaly_sr_describe_texture(void*, rsf_sr_texture*) noexcept;
void rsf_anomaly_sr_describe_view(void*, uint32_t, rsf_sr_view*) noexcept;
uint32_t rsf_anomaly_sr_format_support(void*, uint32_t) noexcept;
void* rsf_anomaly_sr_texture(void*, const rsf_sr_texture*, const void*, uint32_t) noexcept;
void* rsf_anomaly_sr_view(void*, void*, uint32_t, const rsf_sr_view*) noexcept;
int32_t rsf_anomaly_sr_compile_shader(void*, uint32_t, uint32_t, const char*,
    bool (*)(void*, const char*), void*, void**) noexcept;
void* rsf_anomaly_sr_constant_buffer(void*) noexcept;
void* rsf_anomaly_sr_raster_state(void*) noexcept;
void* rsf_anomaly_sr_blend_state(void*) noexcept;
void* rsf_anomaly_sr_depth_state(void*, uint32_t) noexcept;
void rsf_anomaly_sr_update_constants(void*, void*, const uint32_t*) noexcept;
void rsf_anomaly_sr_ps_constant(void*, uint32_t, void*) noexcept;
void rsf_anomaly_sr_targets(void*, uint32_t, void* const*, void*) noexcept;
void rsf_anomaly_sr_ps_resources(void*, uint32_t, uint32_t, void* const*) noexcept;
void rsf_anomaly_sr_clear_stencil(void*, void*) noexcept;
void rsf_anomaly_sr_draw(void*, const rsf_sr_draw*) noexcept;
void rsf_anomaly_sr_set_full_viewport(void*, uint32_t, uint32_t) noexcept;

// Stage query and readback leaves. Capture owns the map lifetime.
void* rsf_anomaly_sr_timer_retain_device(void*) noexcept;
void* rsf_anomaly_sr_timer_create_query(void*, bool) noexcept;
void rsf_anomaly_sr_timer_issue(void*, void*, bool) noexcept;
int32_t rsf_anomaly_sr_timer_clock(void*, void*, uint64_t*, bool*) noexcept;
int32_t rsf_anomaly_sr_timer_timestamp(void*, void*, uint64_t*) noexcept;
bool rsf_anomaly_sr_immediate_context(void*) noexcept;
void* rsf_anomaly_sr_context_device(void*) noexcept;
void rsf_anomaly_sr_copy_subresource(void*, void*, uint32_t, void*, uint32_t) noexcept;
bool rsf_anomaly_sr_map_read(void*, void*, uint32_t, const void**, uint32_t*) noexcept;
void rsf_anomaly_sr_unmap(void*, void*, uint32_t) noexcept;
uint32_t rsf_anomaly_sr_twin_refs(void*) noexcept;

// The swap host serializes access. Pass and depth pointers borrow the current resources.
uint32_t rsf_anomaly_sr_stage_make_twins(rsf_sr_stage_resources**, const rsf_sr_d3d11*, void*, const rsf_anomaly_swap_request*,
    rsf_anomaly_swap_twins*, rsf_sr_twin_batch_info*, rsf_sr_log, void*) noexcept;
void rsf_anomaly_sr_stage_retire_twins(rsf_sr_stage_resources*, uint32_t (*refs)(void*)) noexcept;
void rsf_anomaly_sr_stage_reset_pass(rsf_sr_stage_resources*) noexcept;
void rsf_anomaly_sr_stage_drop_resources(rsf_sr_stage_resources*) noexcept;
const rsf_sr_pass* rsf_anomaly_sr_stage_pass(const rsf_sr_stage_resources*) noexcept;
void* rsf_anomaly_sr_stage_depth(const rsf_sr_stage_resources*) noexcept;
void rsf_anomaly_sr_stage_resource_stats(const rsf_sr_stage_resources*, uint64_t*, uint64_t*) noexcept;
uint8_t rsf_anomaly_sr_make_twin(const rsf_sr_d3d11*, void*, const rsf_sr_twin_input*, rsf_sr_target**, rsf_sr_target_view*) noexcept;
uint8_t rsf_anomaly_sr_make_depth(const rsf_sr_d3d11*, void*, void*, rsf_sr_extent, rsf_sr_extent, rsf_sr_target**,
    rsf_sr_target_view*) noexcept;
uint8_t rsf_anomaly_sr_make_scratch(const rsf_sr_d3d11*, void*, void*, rsf_sr_extent, rsf_sr_target**, rsf_sr_target_view*) noexcept;
void rsf_anomaly_sr_drop_target(rsf_sr_target*) noexcept;
void rsf_anomaly_sr_twin_fields(const rsf_sr_target_view*, rsf_anomaly_swap_fields*) noexcept;
uint8_t rsf_anomaly_sr_make_temporal(const rsf_sr_d3d11*, void*, rsf_sr_extent, rsf_sr_extent, uint32_t, rsf_sr_temporal_targets**,
    rsf_sr_temporal_view*) noexcept;
void rsf_anomaly_sr_drop_temporal(rsf_sr_temporal_targets*) noexcept;
rsf_sr_pass* rsf_anomaly_sr_create_pass(const rsf_sr_d3d11*, void*, uint32_t, rsf_sr_log, void*) noexcept;
void rsf_anomaly_sr_drop_pass(rsf_sr_pass*) noexcept;
void* rsf_anomaly_sr_pass_device(const rsf_sr_pass*) noexcept;
uint32_t rsf_anomaly_sr_temporal_ready(const rsf_sr_pass*) noexcept;
uint32_t rsf_anomaly_sr_run_pass(const rsf_sr_pass*, void*, uint32_t, const rsf_sr_draw_views*) noexcept;
}
