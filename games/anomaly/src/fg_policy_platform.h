// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_frame.h>

enum {
    RSF_ANOMALY_SKIP_NONE, RSF_ANOMALY_SKIP_NO_LEVEL, RSF_ANOMALY_SKIP_SCOPE,
    RSF_ANOMALY_SKIP_NO_PRESENT, RSF_ANOMALY_SKIP_NO_INPUTS, RSF_ANOMALY_SKIP_COUNT,
};
enum {
    RSF_ANOMALY_RESET_NONE, RSF_ANOMALY_RESET_FIRST, RSF_ANOMALY_RESET_DEVICE,
    RSF_ANOMALY_RESET_DISCONTINUITY, RSF_ANOMALY_RESET_SCOPE_END, RSF_ANOMALY_RESET_SIZE,
    RSF_ANOMALY_RESET_CUT, RSF_ANOMALY_RESET_COUNT,
};
enum {
    RSF_ANOMALY_GAMEPLAY_SCREEN = RSF_SCREEN_FLIGHT,
    RSF_ANOMALY_HELD_SCREEN = RSF_SCREEN_UNKNOWN,
};

typedef struct rsf_anomaly_fg_decision {
    uint32_t tag, screen;
    int32_t reset, skip;
} rsf_anomaly_fg_decision;

typedef struct rsf_anomaly_fg_counters {
    uint64_t source_frames, level_frames, eligible_at_ui_boundary, fed, presented, unpresented, scope_frames;
    uint64_t skipped[RSF_ANOMALY_SKIP_COUNT], resets[RSF_ANOMALY_RESET_COUNT];
} rsf_anomaly_fg_counters;
