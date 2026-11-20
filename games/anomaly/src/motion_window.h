// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stdint.h>

typedef struct rsf_anomaly_motion_window {
    void *context, *position_rtv, *position_surface;
    uint64_t generation;
    uint32_t frame;
    uint8_t view;
    bool open, have_frame, target_ok;
} rsf_anomaly_motion_window;

enum { RSF_ANOMALY_DRAW_REPLAY = 1, RSF_ANOMALY_DRAW_RESTORE = 2 };

typedef struct rsf_anomaly_motion_draw {
    void *context, *vs, *ps, *hs, *ds;
    int32_t kind;
    bool tessellated;
} rsf_anomaly_motion_draw;

#ifdef __cplusplus
extern "C" {
#endif
bool rsf_anomaly_motion_window_begin(rsf_anomaly_motion_window*, void* context, void* rtv, void* surface,
    uint32_t frame, uint8_t view, uint64_t generation);
void rsf_anomaly_motion_window_open(rsf_anomaly_motion_window*, bool target_ok);
bool rsf_anomaly_motion_window_close(rsf_anomaly_motion_window*);
void rsf_anomaly_motion_window_reset(rsf_anomaly_motion_window*);
void rsf_anomaly_motion_draw_arm(const rsf_anomaly_motion_draw*);
// A null context cancels any pending action on the render thread.
bool rsf_anomaly_motion_draw_take(void* context, rsf_anomaly_motion_draw*);
#ifdef __cplusplus
}
#endif
