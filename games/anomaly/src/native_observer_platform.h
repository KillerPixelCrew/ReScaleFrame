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

// Native actor order is the same as the two verified hook banks.
#define RSF_ANOMALY_OBSERVER_BEGIN 0
#define RSF_ANOMALY_OBSERVER_END 1
#define RSF_ANOMALY_OBSERVER_CAMERA 2
#define RSF_ANOMALY_OBSERVER_SCENE 3
#define RSF_ANOMALY_OBSERVER_UI 4
#define RSF_ANOMALY_OBSERVER_TARGET_COPY 5
#define RSF_ANOMALY_OBSERVER_RESET_BEGIN 6
#define RSF_ANOMALY_OBSERVER_RESET_END 7

typedef struct rsf_anomaly_observer_binding {
    void* replacement;
    void** original;
} rsf_anomaly_observer_binding;

typedef struct rsf_anomaly_observer_engine {
    void* image;
    size_t build;
    const char* label;
    const char* executable;
} rsf_anomaly_observer_engine;

typedef struct rsf_anomaly_observer_frame {
    uint32_t id, width, height;
} rsf_anomaly_observer_frame;

typedef struct rsf_anomaly_observer_camera {
    float view[16], projection[16];
} rsf_anomaly_observer_camera;

// The C leaves native_observer.rs calls: the observer hook set, the stage installs and the guarded reads.
#ifdef __cplusplus
extern "C" {
#endif
// An immutable refusal reason, or null when the profile and all guards match.
const char* rsf_anomaly_observer_native_identify(rsf_anomaly_observer_engine*);
bool rsf_anomaly_observer_native_enable_hooks(const rsf_anomaly_observer_engine*);
bool rsf_anomaly_observer_native_disable_hooks(void);
bool rsf_anomaly_observer_native_install_geometry(const rsf_anomaly_observer_engine*);
bool rsf_anomaly_observer_native_install_sr(const rsf_anomaly_observer_engine*);
bool rsf_anomaly_observer_native_install_motion(const rsf_anomaly_observer_engine*);
bool rsf_anomaly_observer_native_enable_input(void);
bool rsf_anomaly_observer_native_disable_input(void);
bool rsf_anomaly_observer_native_read_frame(const rsf_anomaly_observer_engine*, rsf_anomaly_observer_frame*);
bool rsf_anomaly_observer_native_copy(void*, const void*, size_t);
#ifdef __cplusplus
}
#endif
