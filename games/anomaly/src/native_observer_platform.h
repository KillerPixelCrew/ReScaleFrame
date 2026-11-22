// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
#include <stdbool.h>
#include <stddef.h>

// The payload of the existing rsf_anomaly_observation export.
#define RSF_ANOMALY_OBSERVATION_ABI 1
typedef struct rsf_anomaly_native_observation {
    uint32_t struct_size, abi_version;
    uint32_t active, width, height, generation;
    uint64_t render_frames, scene_passes, hud_passes, pda_copies, scope_copies, camera_updates, resets;
    uint32_t engine_frame, camera_finite;
    float view[16], projection[16];
} rsf_anomaly_native_observation;

// The immutable projection of one native profile. Site banks are NativeSite
// and UniqueGuard arrays whose layouts are checked by the native and Rust owners.
typedef struct rsf_anomaly_observer_profile {
    const char* label;
    uint32_t image_size, frame_rva, width_rva, height_rva;
    const void* sites;
    size_t site_count;
    const void* unique_guards;
    size_t unique_guard_count;
} rsf_anomaly_observer_profile;

#define RSF_ANOMALY_OBSERVER_HOOK_COUNT 8
typedef struct rsf_anomaly_observer_binding {
    void* replacement;
    void** original;
} rsf_anomaly_observer_binding;

typedef struct rsf_anomaly_observer_engine {
    void* image;
    size_t build;
    const char* label;
    const char* executable;
    const rsf_anomaly_observer_profile* profile;
} rsf_anomaly_observer_engine;

typedef struct rsf_anomaly_observer_frame {
    uint32_t id, width, height;
} rsf_anomaly_observer_frame;

typedef struct rsf_anomaly_observer_camera {
    float view[16], projection[16];
} rsf_anomaly_observer_camera;

typedef struct rsf_anomaly_observer_platform {
    // Returns an immutable refusal reason or null if the profile and all guards match.
    const char* (*identify)(rsf_anomaly_observer_engine*);
    bool (*enable_hooks)(const rsf_anomaly_observer_engine*);
    bool (*disable_hooks)(void);
    bool (*install_geometry)(const rsf_anomaly_observer_engine*);
    bool (*disable_geometry)(void);
    bool (*install_sr)(const rsf_anomaly_observer_engine*);
    bool (*quiesce_sr)(void);
    bool (*install_motion)(const rsf_anomaly_observer_engine*);
    bool (*quiesce_motion)(void);
    bool (*enable_input)(void);
    bool (*disable_input)(void);
    bool (*input_idle)(void);
    bool (*read_frame)(const rsf_anomaly_observer_engine*, rsf_anomaly_observer_frame*);
    bool (*guarded_copy)(void*, const void*, size_t);
    void (*render_begin)(uint32_t, uint32_t, uint32_t);
    void (*sr_frame_end)(void);
    void (*end_before)(void);
    void (*end_after)(void);
    void (*camera)(const float*, const float*);
    void (*ui_boundary)(void);
    void (*scope_frame)(void);
    void (*device_reset)(void);
    void (*log)(rsf_game_log_fn, void*, const char*);
    void (*report_stages)(rsf_game_log_fn, void*);
} rsf_anomaly_observer_platform;
