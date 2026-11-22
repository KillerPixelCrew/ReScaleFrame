// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

enum { RSF_MOTION_CORE, RSF_MOTION_FOLIAGE, RSF_MOTION_PIXEL, RSF_MOTION_TESS };
enum { RSF_MOTION_READY, RSF_MOTION_CREATED, RSF_MOTION_MULTISAMPLED, RSF_MOTION_FAILED };

typedef void (*rsf_anomaly_motion_log)(void*, const char*);
// Every callback is required and the immutable table must outlive the owner.
typedef struct rsf_anomaly_motion_device_platform {
    void (*retain)(void*);
    void (*release)(void*);
    void* (*buffer)(void* device, uint32_t size);
    void* (*zero_shader)(void* device, rsf_anomaly_motion_log, void* user);
    void* (*depth_state)(void* device, uint32_t equal);
    void* (*texture)(void* device, uint32_t width, uint32_t height);
    void* (*view)(void* device, void* texture);
    uint32_t (*upload)(void* context, void* buffer, const void* data, size_t size);
    uint32_t (*bind)(void* context, void* buffer, char stage);
} rsf_anomaly_motion_device_platform;

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
typedef struct rsf_anomaly_motion_replay_platform {
    void (*release)(void*);
    void (*read_depth)(void*, rsf_anomaly_motion_replay_depth*);
    void (*read_pixel)(void*, rsf_anomaly_motion_replay_pixel*);
    void (*targets)(void*, uint32_t, void* const*, void*);
    void (*depth)(void*, void*, uint32_t);
    void (*blend)(void*, void*, const float*, uint32_t);
    void (*shader)(void*, void*, void* const*, uint32_t);
} rsf_anomaly_motion_replay_platform;
// The replay owner is initialized only when begin returns 0. End consumes it.
typedef struct rsf_anomaly_motion_replay_scope {
    uint64_t storage[273];
} rsf_anomaly_motion_replay_scope;

#ifdef __cplusplus
extern "C" {
#endif
void rsf_anomaly_motion_clear_target(void* context, void* target);
void rsf_anomaly_motion_bind_targets(void* context, void* const* targets, void* depth);
void rsf_anomaly_motion_clear_targets(void* context);
const rsf_anomaly_motion_device_platform* rsf_anomaly_motion_device_platform_get(void);
const rsf_anomaly_motion_replay_platform* rsf_anomaly_motion_replay_platform_get(void);
void* rsf_anomaly_motion_device_new(const rsf_anomaly_motion_device_platform*);
void rsf_anomaly_motion_device_drop(void*);
void rsf_anomaly_motion_device_reset(void*);
uint32_t rsf_anomaly_motion_device_create(void*, void* device, rsf_anomaly_motion_log, void* user);
int32_t rsf_anomaly_motion_device_target(void*, uint32_t width, uint32_t height, uint32_t samples, uint32_t layers);
void rsf_anomaly_motion_device_get(const void*, rsf_anomaly_motion_device_view*);
uint32_t rsf_anomaly_motion_device_upload(void*, void* context, int32_t kind, const void* data, size_t size);
uint8_t rsf_anomaly_motion_replay_begin(const rsf_anomaly_motion_replay_platform*, const rsf_anomaly_motion_device_view*,
    void* context, rsf_anomaly_motion_replay_scope*);
void rsf_anomaly_motion_replay_end(rsf_anomaly_motion_replay_scope*);
#ifdef __cplusplus
}
#endif
