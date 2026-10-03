// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_api.h>

#define RSF_UNITY_BRIDGE_ABI_VERSION 1u
#define RSF_UNITY_RENDER_EVENT 0x52534601

// Copied at graph execution. All texture pointers are Unity D3D12 resources, borrowed until
// enqueue copies the packet and acquires leases. No managed object crosses this boundary.
typedef struct rsf_unity_packet {
    uint32_t struct_size, abi_version;
    uint64_t session_id, frame_id, view_key;
    uint32_t generation, flags;
    void* color;
    void* depth;
    void* motion;
    void* output;
    rsf_camera_frame camera;
    float previous_clip_to_clip[16];
} rsf_unity_packet;

typedef struct rsf_unity_config {
    uint32_t struct_size, abi_version, enabled, backend, quality, generation;
    uint32_t render_width, render_height, output_width, output_height;
} rsf_unity_config;

typedef struct rsf_unity_native_api {
    uint32_t struct_size, abi_version;
    uint64_t session_id;
    void (*log)(const char* message);
    int (*config)(uint32_t output_width, uint32_t output_height, rsf_unity_config* output);
    void* (*enqueue)(const rsf_unity_packet* packet);
    void* render_event;
    void (*managed_state)(uint32_t stage);
} rsf_unity_native_api;
