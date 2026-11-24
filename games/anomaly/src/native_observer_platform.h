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
