/* SPDX-License-Identifier: MIT */
#pragma once
/* The private boundary between the Rust SR stage driver (sr_stage.rs) and its native leaves (sr_stage.cpp).
   Rust owns the lifecycle, view admission, frame preparation and seam ordering, jitter and camera publication,
   capture and burst bookkeeping, the retained configuration (INI path, vendor directories, live options), status,
   the swap owner and the temporal driver. The leaves keep guarded engine reads, the stage hooks and trampolines, the
   guarded engine calls behind the swap owner and the D3D11 host's resource handle.
   Every callback runs on the calling thread. The Rust stage lock serializes the native controller. */
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "sr_swap_platform.h"

struct rsf_game_render_pass;

typedef void (*rsf_anomaly_sr_log)(void*, const char*);

// Adapter settings supplied to the temporal driver.
typedef struct rsf_anomaly_sr_settings {
    bool enabled;
    uint32_t backend, quality, preset;
} rsf_anomaly_sr_settings;

// The [Anomaly] SR keys sr_stage.rs reads (sr_stage.h describes the stages). The Rust stage also keeps the INI path
// and the vendor runtime directories; this record is its scalar part, copied out for the contract fixtures.
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

/* The published jitter (sr_stage.h), one Halton sample and its viewport-UV form. */
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

/* The swap owner's frame facts. Rust fills admission, sizes, jitter and bias from the native reads. */
typedef rsf_anomaly_swap_facts rsf_anomaly_sr_frame_facts;

typedef struct rsf_anomaly_sr_frame_reads {
    rsf_anomaly_sr_frame_facts facts;
    uint32_t render_view, options_word3;
    bool view_read, options_read;
} rsf_anomaly_sr_frame_reads;

// What the stage reads back from the Rust swap owner after each call.
typedef struct rsf_anomaly_sr_controller {
    bool window_open, refused, jitter_applied, mip_bias_applied;
    uint32_t stage, render_width, render_height;
    float window_mip_bias, saved_mip_bias;
    uint64_t twin_generation;
    const char* refusal;
} rsf_anomaly_sr_controller;

typedef rsf_anomaly_swap_counters rsf_anomaly_sr_swap_counters;

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

typedef struct rsf_anomaly_sr_reset_facts {
    bool first, device, settings, sequence, cut;
} rsf_anomaly_sr_reset_facts;

#define RSF_ANOMALY_SR_MAIN_VIEW_KEY 1u
#define RSF_ANOMALY_SR_MOTION_TO_UV 1.0f
#define RSF_ANOMALY_SR_MOTION_CAMERA_INCLUDED 0u
#define RSF_ANOMALY_SR_MOTION_VALIDITY_CHANNEL 2u
#define RSF_ANOMALY_SR_MOTION_VALIDITY_THRESHOLD 0.5f
#define RSF_ANOMALY_SR_MOTION_SENTINEL (-1000.0f)
#define RSF_ANOMALY_SR_ENGINE_VERSION "X-Ray Monolith 1.5.3 (S.T.A.L.K.E.R. Anomaly, DX11)"
#define RSF_ANOMALY_SR_PROJECT_ID "8d3b6f52-1c4e-4f7a-b0d9-2e6a51c7f3a4"
#define RSF_ANOMALY_SR_VIEW_SPACE_TO_METERS 1.0f
/* DLSS, FSR2, FSR3, FSR4, XeSS and FSR1: every backend id the Runtime's admission can offer. It narrows
   the providers to this machine's packages and GPU; FSR1 is the Runtime's own EASU pass. */
#define RSF_ANOMALY_SR_ADAPTER_BACKENDS 0x7eu
/* Native to Ultra Quality: X-Ray renders any reduced extent; which levels a backend serves is the Runtime's. */
#define RSF_ANOMALY_SR_ADAPTER_QUALITIES 0x3fu

typedef struct rsf_anomaly_sr_motion_target {
    void* texture;
    bool written, main;
    uint32_t frame, width, height;
} rsf_anomaly_sr_motion_target;

typedef struct rsf_anomaly_sr_timer {
    double world_ms[2], seam_ms;
    uint64_t samples[2], seam_samples, skipped;
    bool device;
} rsf_anomaly_sr_timer;

// Temporal driver setup for Stages 4 and 5. The UTF-8 vendor directories are borrowed for the call.
typedef struct rsf_anomaly_sr_driver_config {
    uint32_t stage;
    bool exposure_auto, colour_transport, first_person_zero, require_signature;
    rsf_anomaly_sr_settings initial;
    const char* streamline;
    const char* fidelityfx;
    const char* xess;
} rsf_anomaly_sr_driver_config;

/* The C11 leaf (sr_stage_platform.c) keeps what Rust cannot express; sr_stage/native.rs calls these by name and reads
   every engine address from the selected build's profile records (sr_profile.rs, native_profile.rs). The trampolines
   call back into the Rust stage. */
#define RSF_ANOMALY_SR_DETOUR_COUNT 5   /* phase_scene_prepare, phase_combine, phase_sunshafts, phase_blur, sun cascades */
#ifdef __cplusplus
extern "C" {
#endif

// R_xforms::set_P(this, rdx = matrix), dx10SamplerStateCache::SetMipLODBias(this, xmm1 = bias) and
// R_constants::GetCBuffer(this, constant, stage) at `function`, each under SEH (false or 0 on a fault).
bool rsf_anomaly_sr_native_set_projection(void* function, void* xforms, const void* matrix);
bool rsf_anomaly_sr_native_set_mip_bias(void* function, void* cache, float bias);
uintptr_t rsf_anomaly_sr_native_constant_buffer(void* function, void* constants, uintptr_t constant, uint32_t stage);

// The cl_pos_decompress_params::setup vtable replacement (x64, so the engine's __fastcall is the C convention).
void rsf_anomaly_sr_position_setup_detour(void* self, void* constant);

// Rust entries of the leaf.
void rsf_anomaly_sr_stage_prepare(void* target);
void rsf_anomaly_sr_stage_prepared(void);
void rsf_anomaly_sr_stage_seam(void* target);
void rsf_anomaly_sr_stage_combine_end(void);
bool rsf_anomaly_sr_stage_sun_begin(void);
void rsf_anomaly_sr_stage_sun_end(bool suspended);
void rsf_anomaly_sr_stage_position_setup(void* constant);
#ifdef __cplusplus
}
#endif

// Live panel options, in registration order (sr_stage.rs registers them with the overlay service).
#define RSF_ANOMALY_SR_OPTION_COLOUR_TRANSPORT 0
#define RSF_ANOMALY_SR_OPTION_EXPOSURE_AUTO 1
#define RSF_ANOMALY_SR_OPTION_FIRST_PERSON_ZERO 2
#define RSF_ANOMALY_SR_OPTION_JITTER 3
#define RSF_ANOMALY_SR_OPTION_MIP_BIAS 4
#define RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET 5
