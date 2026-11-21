// SPDX-License-Identifier: MIT
#pragma once
// The private boundary between the Rust SR stage driver (sr_stage.rs) and its native leaves (sr_stage.cpp).
// Rust owns the lifecycle, view admission, frame preparation and seam ordering, jitter and camera publication,
// capture and burst bookkeeping, options and status. The leaves keep guarded engine reads, the stage hooks and
// trampolines, the SwapController and its D3D11 host, and the C++ TemporalDriver facade.
// Every callback runs on the calling thread; none of them re-enters the Rust stage lock except the seam passes,
// which receive the locked state as `cookie` from the seam or prepare call that holds it.
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

struct rsf_camera_frame;
struct rsf_game_render_pass;

typedef void (*rsf_anomaly_sr_log)(void*, const char*);

// SrSettings (sr_settings.h).
typedef struct rsf_anomaly_sr_settings {
    bool enabled;
    uint32_t backend, quality, preset;
} rsf_anomaly_sr_settings;

// The scalar part of StageOptions (sr_stage.h). Paths stay with the native options.
typedef struct rsf_anomaly_sr_stage_config {
    uint32_t stage;
    float scale;
    bool scale_defaulted, jitter, mip_bias;
    float mip_bias_offset;
    bool capture;
    uint32_t capture_start, capture_count, capture_interval;
    bool exposure_auto, colour_transport, first_person_zero, require_signature;
    float cut_distance, cut_degrees;
    rsf_anomaly_sr_settings settings;
} rsf_anomaly_sr_stage_config;

// JitterState, JitterSample and JitterUv (sr_jitter.h), ScalePlan (sr_plan.h).
typedef struct rsf_anomaly_sr_jitter_state {
    uint64_t frame;
    uint32_t sequence_index, phase_count;
    float current_x, current_y, previous_x, previous_y;
    uint32_t render_width, render_height, previous_render_width, previous_render_height;
    bool scope, applied, previous_applied, reset;
} rsf_anomaly_sr_jitter_state;
typedef struct rsf_anomaly_sr_jitter_sample {
    float x, y;
    uint32_t index, phase_count;
} rsf_anomaly_sr_jitter_sample;
typedef struct rsf_anomaly_sr_jitter_uv {
    float current_u, current_v, previous_u, previous_v;
} rsf_anomaly_sr_jitter_uv;
typedef struct rsf_anomaly_sr_plan {
    uint32_t output_width, output_height, render_width, render_height;
    float scale;
} rsf_anomaly_sr_plan;

#define RSF_ANOMALY_SR_MIN_SCALE 0.33f
#define RSF_ANOMALY_SR_MAX_SCALE 1.0f
#define RSF_ANOMALY_SR_DEFAULT_SCALE 0.67f
#define RSF_ANOMALY_SR_MIN_MIP_OFFSET (-2.0f)
#define RSF_ANOMALY_SR_MAX_MIP_OFFSET 1.0f
#define RSF_ANOMALY_SR_DEFAULT_MIP_OFFSET 0.0f
#define RSF_ANOMALY_SR_BURST_SETTLE 30u

typedef struct rsf_anomaly_sr_jitter_sequence {
    uint64_t generation, frames, resets;
    uint32_t dimensions[4], phases, index;
    bool started, reset;
} rsf_anomaly_sr_jitter_sequence;
typedef struct rsf_anomaly_sr_burst_gate {
    uint64_t mark, clear, held, restarts;
    uint32_t settle;
    bool marked;
} rsf_anomaly_sr_burst_gate;
typedef struct rsf_anomaly_sr_burst_verdict {
    bool capture, discard;
} rsf_anomaly_sr_burst_verdict;

#ifdef __cplusplus
extern "C" {
#endif
float rsf_anomaly_sr_clamp_scale(float);
uint32_t rsf_anomaly_sr_render_extent(uint32_t output, float scale);
bool rsf_anomaly_sr_make_plan(uint32_t output_width, uint32_t output_height, float scale, rsf_anomaly_sr_plan*);
bool rsf_anomaly_sr_plan_for_render(uint32_t output_width, uint32_t output_height, uint32_t render_width,
    uint32_t render_height, rsf_anomaly_sr_plan*);
float rsf_anomaly_sr_clamp_mip_offset(float);
float rsf_anomaly_sr_mip_delta(uint32_t render_width, uint32_t output_width, float offset);
double rsf_anomaly_sr_halton(uint32_t index, uint32_t base);
uint32_t rsf_anomaly_sr_phase_count(uint32_t render_width, uint32_t output_width);
rsf_anomaly_sr_jitter_sample rsf_anomaly_sr_jitter_sample_at(uint32_t index, uint32_t phases);
rsf_anomaly_sr_jitter_sample rsf_anomaly_sr_sequence_next(rsf_anomaly_sr_jitter_sequence*,
    const uint32_t (*dimensions)[4], uint64_t generation, bool continuous);
void rsf_anomaly_sr_next_jitter(const rsf_anomaly_sr_jitter_state*, bool scope, bool applied,
    const rsf_anomaly_sr_jitter_sample*, uint32_t render_width, uint32_t render_height, bool reset, rsf_anomaly_sr_jitter_state*);
void rsf_anomaly_sr_jitter_to_uv(const rsf_anomaly_sr_jitter_state*, rsf_anomaly_sr_jitter_uv*);
rsf_anomaly_sr_burst_verdict rsf_anomaly_sr_burst_frame(rsf_anomaly_sr_burst_gate*, bool loading, uint64_t without_world, bool taken);
void rsf_anomaly_sr_burst_interrupted(rsf_anomaly_sr_burst_gate*);
#ifdef __cplusplus
}
#endif

// FrameFacts (sr_swap.h). The leaf fills the engine reads and addresses; Rust fills sizes, jitter and bias.
typedef struct rsf_anomaly_sr_frame_facts {
    void* render_target;
    void* backend;
    uint64_t device, generation;
    uint32_t device_width, device_height;
    bool scope, msaa;
    uint32_t stage, render_width, render_height;
    void* device_size;
    void* base_zb;
    void* base_rt;
    void* ssao_viewport;
    void* ssao_viewport_guard;
    bool jitter;
    float jitter_x, jitter_y;
    void* device_view;
    void* device_project;
    void* device_full_transform;
    bool mip_bias;
    float mip_bias_delta;
    void* sampler_bias;
} rsf_anomaly_sr_frame_facts;

// What the stage reads back from the SwapController after each call.
typedef struct rsf_anomaly_sr_controller {
    bool window_open, refused, jitter_applied, mip_bias_applied;
    uint32_t stage, render_width, render_height;
    float window_mip_bias, saved_mip_bias;
    uint64_t twin_generation;
    const char* refusal;
} rsf_anomaly_sr_controller;

#define RSF_ANOMALY_SR_SKIPS 6
#define RSF_ANOMALY_SR_RESTORES 6
// SwapCounters (sr_swap.h), copied field by field. Skip and Restore keep their enum order.
typedef struct rsf_anomaly_sr_swap_counters {
    uint64_t prepares, eligible, swapped, seams, seams_without_window, round_trips, round_trip_failures;
    uint64_t combine_ends, rm_near_in_window, twin_generations, twin_drops, foreign_changes, write_faults;
    uint64_t allocation_failures, viewports, ssao_viewport_swaps;
    uint64_t jittered, jitter_invalid, fp_jittered, fp_already, fp_foreign, projection_restores;
    uint64_t mip_applied, mip_restored, mip_unchanged;
    uint64_t sun_calls, sun_unjittered, sun_restored, sun_refused;
    uint64_t position_corrected, position_stock, position_refused;
    uint64_t skipped[RSF_ANOMALY_SR_SKIPS];
    uint64_t restores[RSF_ANOMALY_SR_RESTORES];
} rsf_anomaly_sr_swap_counters;

// Device.mView and the unjittered Device.mProject (row-vector engine matrices) at a main-view H1.
typedef struct rsf_anomaly_sr_camera {
    float view[16], projection[16];
    bool valid;
} rsf_anomaly_sr_camera;

// Stage 5: frame facts to pair with the texture leases held by the native seam.
typedef struct rsf_anomaly_sr_pass {
    uint64_t native_frame;
    uint32_t generation, render_width, render_height, output_width, output_height;
    rsf_anomaly_sr_camera current, previous;
    rsf_anomaly_sr_jitter_state jitter;
    float frame_seconds;
    bool motion_missing, exposure_auto, reset_sequence, cut;
} rsf_anomaly_sr_pass;

typedef struct rsf_anomaly_sr_pass_inputs {
    rsf_anomaly_sr_pass frame;
    uint64_t session;
    bool reset;
    void *color, *color_output, *depth, *motion, *exposure;
} rsf_anomaly_sr_pass_inputs;

typedef struct rsf_anomaly_sr_planes {
    float near_plane, far_plane, vertical_fov;
    bool valid;
} rsf_anomaly_sr_planes;
typedef struct rsf_anomaly_sr_pose {
    double position[3], forward[3];
    bool valid;
} rsf_anomaly_sr_pose;
typedef struct rsf_anomaly_sr_reset_facts {
    bool first, device, settings, sequence, cut;
} rsf_anomaly_sr_reset_facts;

#define RSF_ANOMALY_SR_MAIN_VIEW_KEY 1u
#define RSF_ANOMALY_SR_MOTION_TO_UV 1.0f
#define RSF_ANOMALY_SR_MOTION_CAMERA_INCLUDED 0u

#ifdef __cplusplus
extern "C" {
#endif
uint8_t rsf_anomaly_sr_build_pass(const rsf_anomaly_sr_pass_inputs*, struct rsf_game_render_pass*);
void rsf_anomaly_sr_projection_planes(const float (*projection)[16], rsf_anomaly_sr_planes*);
void rsf_anomaly_sr_pose_of(const float (*view)[16], rsf_anomaly_sr_pose*);
bool rsf_anomaly_sr_camera_cut(const rsf_anomaly_sr_pose*, const rsf_anomaly_sr_pose*, float distance, float degrees);
uint8_t rsf_anomaly_sr_reset_reason(const rsf_anomaly_sr_reset_facts*);
#ifdef __cplusplus
}
#endif

// The S-B seam the controller is closing: the engine and twin generic0 surfaces and the window sizes.
typedef struct rsf_anomaly_sr_seam {
    void* twin_surface;
    void* engine_surface;
    uint32_t render_width, render_height, output_width, output_height;
} rsf_anomaly_sr_seam;

typedef struct rsf_anomaly_sr_motion_target {
    void* texture;
    bool written, main;
    uint32_t frame, width, height;
} rsf_anomaly_sr_motion_target;

typedef struct rsf_anomaly_sr_temporal_textures {
    void* colour;
    void* depth;
    void* motion;
    void* output;
} rsf_anomaly_sr_temporal_textures;

typedef struct rsf_anomaly_sr_timer {
    double world_ms[2], seam_ms;
    uint64_t samples[2], seam_samples, skipped;
    bool device;
} rsf_anomaly_sr_timer;

typedef struct rsf_anomaly_sr_stage_platform {
    // Guarded engine reads and engine identity.
    void (*frame_facts)(void* self, bool world_window, bool temporal, rsf_anomaly_sr_frame_facts* out);
    bool (*loading_screen)(void);
    uint32_t (*engine_frame)(void);
    void (*device_size)(uint32_t* width, uint32_t* height);
    bool (*read_camera)(rsf_anomaly_sr_camera* out);
    void* (*seam_surface)(void* self);
    void* (*back_buffer)(void);   // an owned reference released through `release`
    void* (*context)(void);
    void* (*device)(void);
    void (*now)(int64_t* counter, int64_t* frequency);
    bool (*geometry_active)(void);
    const char* (*build_sha256)(void);
    // SwapController (sr_swap.h).
    void (*close_stale)(void);
    bool (*prepare)(const rsf_anomaly_sr_frame_facts*);
    void (*prepared)(void);
    bool (*seam)(void* cookie);
    void (*combine_end)(void);
    void (*frame_end)(void);
    void (*rm_near)(void);
    bool (*sun_begin)(void);
    void (*sun_end)(bool suspended);
    void (*position_setup)(void* constant);
    void (*shutdown)(void);
    void (*set_swap_enabled)(bool);
    void (*controller)(rsf_anomaly_sr_controller*);
    void (*swap_counters)(rsf_anomaly_sr_swap_counters*);
    // GPU timestamps of the world window and the seam pass.
    void (*timer_begin)(void);
    void (*timer_seam)(bool swapped);
    void (*timer_finish)(void);
    void (*timer_reset)(void);
    void (*timer_sample)(rsf_anomaly_sr_timer*);
    // D3D11 host. The seam calls act on the seam the controller is closing.
    void (*host_stats)(uint64_t* twin_bytes, uint64_t* drop_refs);
    void (*host_reset)(void);
    void* (*staging_copy)(void* context, void* source);   // an owned staging texture or null
    void (*release)(void*);
    bool (*write_raw)(const uint16_t* path, void* context, void* texture, uint32_t* width, uint32_t* height, uint32_t* format);
    bool (*bilinear)(void);
    bool (*round_trip)(void);
    bool (*temporal_ready)(void);
    int32_t (*temporal_targets)(uint32_t format);   // 0 kept, 1 created, -1 refused
    bool (*motion_target)(rsf_anomaly_sr_motion_target*);
    bool (*motion_view)(void* texture);
    bool (*seam_prepare)(bool motion_present, bool first_person_zero);
    void (*temporal_textures)(rsf_anomaly_sr_temporal_textures*);
    bool (*reinsert)(void);
    // TemporalDriver (sr_temporal.h).
    bool (*driver_bind)(void);
    bool (*driver_enabled)(void);
    uint32_t (*driver_output_format)(void);
    bool (*driver_planned)(uint32_t output_width, uint32_t output_height, uint32_t* render_width, uint32_t* render_height);
    void (*driver_boundary)(uint32_t output_width, uint32_t output_height);
    bool (*driver_evaluate)(const rsf_anomaly_sr_pass*, struct rsf_camera_frame*, uint32_t*);
    bool (*driver_request_dump)(const char* prefix_utf8);
    void (*driver_count)(uint8_t refusal);
    void (*driver_count_bilinear)(void);
    void (*driver_count_prepared)(bool motion_missing);
    void (*driver_stop)(void);
    void (*driver_report)(rsf_anomaly_sr_log, void*);
    void (*driver_overlay_report)(void* report);
    void (*driver_request)(const rsf_anomaly_sr_settings*);
    // Install and quiesce.
    const char* (*verify_guards)(void* module, uint32_t image_size, const void* build, bool world_window, bool temporal);
    bool (*hooks_created)(void);
    void (*set_engine)(void* module, const void* build, bool temporal);
    bool (*install_hooks)(void);
    bool (*install_position_slot)(void);
    void (*disable_hooks)(void);
    void (*set_rm_near)(bool);
    bool (*remove_hooks)(void);
    // Options: the INI, persistence through the adapter option path and the vendor runtime directories.
    int32_t (*ini_int)(const void* ini, const char* key, int32_t fallback);
    uint32_t (*ini_text)(const void* ini, const char* key, const char* fallback, uint16_t* out, uint32_t capacity);
    const void* (*configured_ini)(void);
    void (*persist_int)(const char* key, int32_t value);
    void (*persist_text)(const char* key, const char* value);
    void (*apply_colour_transport)(bool);
    bool (*vendor_file)(uint32_t directory, const char* name);   // 0 Streamline, 1 FidelityFX, 2 XeSS
} rsf_anomaly_sr_stage_platform;

// Live options, in registration order.
#define RSF_ANOMALY_SR_OPTION_COLOUR_TRANSPORT 0
#define RSF_ANOMALY_SR_OPTION_EXPOSURE_AUTO 1
#define RSF_ANOMALY_SR_OPTION_FIRST_PERSON_ZERO 2
#define RSF_ANOMALY_SR_OPTION_JITTER 3
#define RSF_ANOMALY_SR_OPTION_MIP_BIAS 4
#define RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET 5
