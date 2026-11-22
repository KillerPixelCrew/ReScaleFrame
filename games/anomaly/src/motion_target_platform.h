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

// Every callback is required and the table is immutable for each call.
typedef struct rsf_anomaly_motion_bind_platform {
    uint32_t (*read)(void* host, const void* address, void* out, size_t);
    uint32_t (*write)(void* host, void* address, const void* in, size_t);
    void (*set_targets)(void* host, void* const* targets, void* depth);
    void (*clear_targets)(void* host);
} rsf_anomaly_motion_bind_platform;

#ifdef __cplusplus
extern "C" {
#endif
int32_t rsf_anomaly_motion_bind_draw(rsf_anomaly_motion_bind_state*,
    const rsf_anomaly_motion_bind_platform*, void* host, const rsf_anomaly_motion_bind_layout*,
    void* backend, uint32_t motion_wanted, const void* position_rtv, void* motion_rtv);
int32_t rsf_anomaly_motion_bind_close(rsf_anomaly_motion_bind_state*,
    const rsf_anomaly_motion_bind_platform*, void* host, const rsf_anomaly_motion_bind_layout*,
    void* backend, void* motion_rtv);
#ifdef __cplusplus
}
#endif
