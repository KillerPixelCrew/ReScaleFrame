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

struct rsf_camera_frame;
struct rsf_game_render_pass;
struct rsf_sr_draw_views;

typedef void (*rsf_anomaly_sr_log)(void*, const char*);

/* The verified engine layout and synchronous guarded reads borrowed by constant correction. */
typedef struct rsf_anomaly_position_layout {
    uint32_t destination, load, buffer_size, buffer_data, buffer_changed, buffer_slots;
    uint32_t stage_bits[6], stage_shifts[6];
} rsf_anomaly_position_layout;
typedef struct rsf_anomaly_position_platform {
    void* user;
    bool (*read)(void*, uintptr_t, void*, size_t);
    bool (*write)(void*, uintptr_t, const void*, size_t);
    uintptr_t (*buffer)(void*, uintptr_t, uint32_t);
} rsf_anomaly_position_platform;
#ifdef __cplusplus
static_assert(sizeof(rsf_anomaly_position_layout) == 72 && sizeof(rsf_anomaly_position_platform) == 32,
    "position correction layout");
#else
_Static_assert(sizeof(rsf_anomaly_position_layout) == 72 && sizeof(rsf_anomaly_position_platform) == 32,
    "position correction layout");
#endif

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

#ifdef __cplusplus
extern "C" {
#endif
uint32_t rsf_anomaly_sr_correct_position_constant(
    const rsf_anomaly_position_layout*, const rsf_anomaly_position_platform*, uintptr_t, float, float);
float rsf_anomaly_sr_clamp_mip_offset(float);
float rsf_anomaly_sr_mip_delta(uint32_t render_width, uint32_t output_width, float offset);
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
#define RSF_ANOMALY_SR_ADAPTER_BACKENDS 0x3au
#define RSF_ANOMALY_SR_ADAPTER_QUALITIES 0x1fu

#ifdef __cplusplus
extern "C" {
#endif
uint8_t rsf_anomaly_sr_build_pass(const rsf_anomaly_sr_pass_inputs*, struct rsf_game_render_pass*);
#ifdef __cplusplus
}
#endif

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

typedef struct rsf_anomaly_sr_stage_platform {
    // Guarded engine reads and engine identity.
    void* (*render_target)(void);
    void (*frame_facts)(bool world_window, bool temporal, rsf_anomaly_sr_frame_reads* out);
    bool (*loading_registered)(uint8_t* registered);
    bool (*precache_frame)(uint32_t* frame);
    uint32_t (*engine_frame)(void);
    void (*device_size)(uint32_t* width, uint32_t* height);
    bool (*read_camera)(rsf_anomaly_sr_camera* out);
    void* (*seam_surface)(void* target);
    void* (*back_buffer)(void);   // an owned reference released through `release`
    void* (*context)(void);
    void* (*device)(void);
    void (*now)(int64_t* counter, int64_t* frequency);
    bool (*geometry_active)(void);
    const char* (*build_sha256)(void);
    // Scoped production swap owner (sr_swap.rs).
    void (*close_stale)(void);
    bool (*prepare)(const rsf_anomaly_sr_frame_facts*, rsf_anomaly_sr_log, void* log_user);
    void (*prepared)(void);
    void* (*swap_handle)(void);
    void (*combine_end)(void);
    void (*frame_end)(void);
    void (*rm_near)(void);
    bool (*sun_begin)(void);
    void (*sun_end)(bool suspended);
    void (*position_setup)(void* constant);
    bool (*shutdown)(void);
    void (*set_swap_enabled)(bool);
    void (*controller)(rsf_anomaly_sr_controller*);
    void (*swap_counters)(rsf_anomaly_sr_swap_counters*);
    // GPU timestamp leaves. Device and query creation return owned references.
    void* (*timer_retain_device)(void* device);
    void* (*timer_create_query)(void* device, bool disjoint);
    void (*timer_issue)(void* context, void* query, bool begin);
    int32_t (*timer_clock)(void* context, void* query, uint64_t* frequency, bool* disjoint);
    int32_t (*timer_timestamp)(void* context, void* query, uint64_t* ticks);
    // D3D11 host. The seam calls act on the seam the controller is closing.
    void (*host_stats)(uint64_t* twin_bytes, uint64_t* drop_refs);
    void (*host_reset)(void);
    // Stage 3 burst readback. Rust admits the context, source and device, describes the staging texture through the
    // rsf_sr_d3d11 table and names each subresource; these leaves keep the typed context calls. `copy_subresource`
    // clears predication around CopySubresourceRegion(destination, subresource, 0, 0, 0, source, subresource, null)
    // and restores it. `map_read` is Map(D3D11_MAP_READ, no flags) and `pixel_bytes` the shared DXGI size table.
    bool (*immediate_context)(void* context);
    void* (*context_device)(void* context);   // an owned reference or null
    void (*copy_subresource)(void* context, void* destination, uint32_t destination_subresource, void* source,
        uint32_t source_subresource);
    void (*release)(void*);
    bool (*map_read)(void* context, void* texture, uint32_t subresource, const void** data, uint32_t* row_pitch);
    void (*unmap)(void* context, void* texture, uint32_t subresource);
    uint32_t (*pixel_bytes)(uint32_t format);
    bool (*draw_pass)(uint32_t kind, const struct rsf_sr_draw_views*);
    bool (*temporal_ready)(void);
    const void* (*temporal_platform)(void);   // the static rsf_sr_d3d11 table
    bool (*motion_target)(rsf_anomaly_sr_motion_target*);
    void* (*create_motion_view)(void* device, void* texture);   // an owned reference or null
    void* (*depth_texture)(void);   // borrowed by the seam
    // Rust temporal owner. The production registration fills these slots and synthetic fixtures can supply fakes.
    bool (*driver_bind)(void);
    bool (*driver_enabled)(void);
    uint32_t (*driver_output_format)(void);
    bool (*driver_planned)(uint32_t output_width, uint32_t output_height, uint32_t* render_width, uint32_t* render_height);
    void (*driver_boundary)(uint32_t output_width, uint32_t output_height);
    bool (*driver_evaluate)(rsf_anomaly_sr_pass_inputs*, struct rsf_camera_frame*, uint32_t*);
    bool (*driver_request_dump)(const char* prefix_utf8);
    void (*driver_fallback)(uint8_t reason, bool drawn);
    void (*driver_count_prepared)(bool motion_missing);
    void (*driver_stop)(void);
    void (*driver_report)(bool exposure_auto, bool first_person_zero, rsf_anomaly_sr_log, void*);
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
    // Options: [Anomaly] INI reads (`ini` is a NUL-terminated UTF-16 path), persistence through the adapter option
    // path, the Runtime's colour transport and a file probe for the vendor runtimes (a UTF-16 path).
    int32_t (*ini_int)(const void* ini, const char* key, int32_t fallback);
    uint32_t (*ini_text)(const void* ini, const char* key, const char* fallback, uint16_t* out, uint32_t capacity);
    bool (*persist_int)(const char* key, int32_t value);
    bool (*persist_text)(const char* key, const char* value);
    void (*apply_colour_transport)(bool);
    // An accepted configuration: the log the D3D11 host reports through and, for Stages 4 and 5, the TemporalDriver
    // options (null otherwise).
    void (*configured)(rsf_anomaly_sr_log log, void* user, const rsf_anomaly_sr_driver_config* driver);
    // Calls a native log sink without letting a C++ exception cross into Rust.
    void (*guarded_log)(rsf_anomaly_sr_log log, void* user, const char* message);
} rsf_anomaly_sr_stage_platform;

/* The operations of the C11 leaf (sr_stage_platform.c) that Rust cannot express. sr_stage/native.rs builds the
   production stage table from them and reads every engine address from the selected build's profile records
   (sr_profile.rs, native_profile.rs). The trampolines call back into the Rust stage. */
struct rsf_anomaly_hook_site;
#define RSF_ANOMALY_SR_DETOUR_COUNT 5   /* phase_scene_prepare, phase_combine, phase_sunshafts, phase_blur, sun cascades */
typedef struct rsf_anomaly_sr_native {
    uint32_t struct_size;
    // Guarded engine-memory copy, and a log sink call that contains a foreign C++ exception.
    bool (*copy)(void* output, const void* input, size_t size);
    void (*log)(rsf_anomaly_sr_log log, void* user, const char* message);
    // R_xforms::set_P(this, rdx = matrix), dx10SamplerStateCache::SetMipLODBias(this, xmm1 = bias) and
    // R_constants::GetCBuffer(this, constant, stage) at `function`, each under SEH (false or 0 on a fault).
    bool (*set_projection)(void* function, void* xforms, const void* matrix);
    bool (*set_mip_bias)(void* function, void* cache, float bias);
    uintptr_t (*constant_buffer)(void* function, void* constants, uintptr_t constant, uint32_t stage);
    // Fills each lent site's replacement and trampoline slot in detour order, then installs the batch.
    bool (*install_hooks)(void* image, uint32_t size, struct rsf_anomaly_hook_site* sites, size_t count);
    bool (*hooks_created)(void);
    bool (*enable_hooks)(void);
    bool (*disable_hooks)(void);
    void* position_setup;   // the cl_pos_decompress_params::setup vtable replacement
    bool (*compare_slot)(void* address, void* expected, void* value, void** previous);
    uint8_t (*current_view)(const void* module, size_t build);
    uint32_t (*pixel_bytes)(uint32_t format);
    void* (*create_view)(void* device, void* texture);   // an owned reference or null
    const void* (*d3d11)(void);                           // the process-lifetime rsf_sr_d3d11 table
} rsf_anomaly_sr_native;
#ifdef __cplusplus
static_assert(sizeof(rsf_anomaly_sr_native) == 128, "SR native leaf table");
extern "C" {
#else
_Static_assert(sizeof(rsf_anomaly_sr_native) == 128, "SR native leaf table");
#endif
// Rust entries of the leaf. `ini` is a NUL-terminated UTF-16 path; `build` the native_profile.rs index.
bool rsf_anomaly_sr_stage_configure_native(const rsf_anomaly_sr_native*, const uint16_t* ini, bool apply,
    rsf_anomaly_sr_log log, void* user);
bool rsf_anomaly_sr_stage_install_build(const rsf_anomaly_sr_native*, void* image, size_t build);
void rsf_anomaly_sr_stage_prepare(void* target);
void rsf_anomaly_sr_stage_prepared(void);
void rsf_anomaly_sr_stage_seam(void* target);
void rsf_anomaly_sr_stage_combine_end(void);
bool rsf_anomaly_sr_stage_sun_begin(void);
void rsf_anomaly_sr_stage_sun_end(bool suspended);
void rsf_anomaly_sr_stage_position_setup(void* constant);
// The leaf's table, for the hook fixture.
const rsf_anomaly_sr_native* rsf_anomaly_sr_native_leaves(void);
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
