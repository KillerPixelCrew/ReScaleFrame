// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

enum { RSF_MOTION_CORE, RSF_MOTION_FOLIAGE, RSF_MOTION_PIXEL, RSF_MOTION_TESS };
enum { RSF_MOTION_READY, RSF_MOTION_CREATED, RSF_MOTION_MULTISAMPLED, RSF_MOTION_FAILED };
enum { RSF_MOTION_REPLAY_READY, RSF_MOTION_REPLAY_NO_DEPTH_WRITE, RSF_MOTION_REPLAY_FAILED };

typedef void (*rsf_anomaly_motion_log)(void*, const char*);
typedef struct rsf_anomaly_motion_device_view {
    void* device;
    void* texture;
    void* rtv;
    void* zero_shader;
    void* no_depth;
    void* equal_depth;
    uint32_t width, height;
    uint64_t uploads, upload_skips;
} rsf_anomaly_motion_device_view;

// Bindings returned by read_depth/read_pixel transfer one reference to Rust.
typedef struct rsf_anomaly_motion_replay_depth {
    void* targets[8];
    void* view;
    void* state;
    uint32_t reference;
    bool enabled, writable;
} rsf_anomaly_motion_replay_depth;
typedef struct rsf_anomaly_motion_replay_pixel {
    void* blend;
    float factor[4];
    uint32_t mask;
    void* shader;
    void* classes[253];
    uint32_t count;
} rsf_anomaly_motion_replay_pixel;
// The replay owner is initialized only when begin returns RSF_MOTION_REPLAY_READY. End consumes it.
typedef struct rsf_anomaly_motion_replay_scope {
    uint64_t storage[273];
} rsf_anomaly_motion_replay_scope;

#ifdef __cplusplus
extern "C" {
#endif
void rsf_anomaly_motion_bind_targets(void* context, void* const* targets, void* depth);
void rsf_anomaly_motion_clear_targets(void* context);
typedef struct rsf_anomaly_motion_device_owner rsf_anomaly_motion_device_owner;
rsf_anomaly_motion_device_owner* rsf_anomaly_motion_device_new(void);
uint32_t rsf_anomaly_motion_device_create(rsf_anomaly_motion_device_owner*, void* device, rsf_anomaly_motion_log, void* user);
int32_t rsf_anomaly_motion_device_target(rsf_anomaly_motion_device_owner*, uint32_t width, uint32_t height,
    uint32_t samples, uint32_t layers);
void rsf_anomaly_motion_device_get(const rsf_anomaly_motion_device_owner*, rsf_anomaly_motion_device_view*);
uint32_t rsf_anomaly_motion_device_upload(rsf_anomaly_motion_device_owner*, void* context, int32_t kind,
    const void* data, size_t size);
uint8_t rsf_anomaly_motion_replay_begin(const rsf_anomaly_motion_device_view*, void* context,
    rsf_anomaly_motion_replay_scope*);
void rsf_anomaly_motion_replay_end(rsf_anomaly_motion_replay_scope*);
#ifdef __cplusplus
}
#endif
