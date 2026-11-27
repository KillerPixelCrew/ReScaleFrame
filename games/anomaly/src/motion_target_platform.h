// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <stdint.h>

enum {
    RSF_ANOMALY_MOTION_BIND_NONE,
    RSF_ANOMALY_MOTION_BIND_BOUND,
    RSF_ANOMALY_MOTION_BIND_ALREADY_BOUND,
    RSF_ANOMALY_MOTION_BIND_UNBOUND,
    RSF_ANOMALY_MOTION_BIND_CONFLICT,
    RSF_ANOMALY_MOTION_BIND_FAULT,
};

typedef struct rsf_anomaly_motion_bind_state {
    uint32_t bound, cache_may_hold_motion;
    void* backend;
    void* host;
} rsf_anomaly_motion_bind_state;

typedef struct rsf_anomaly_motion_bind_layout {
    uint32_t rt_offset, zb_offset, changed_offset;
} rsf_anomaly_motion_bind_layout;

