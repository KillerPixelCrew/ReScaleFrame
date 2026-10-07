// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_api.h>

// Internal x64 native/Mono contract mirrored in managed/Native.cs. Struct sizes are exact guards;
// changing field order/width requires a matching ABI bump and managed update.
#define RSF_UNITY_BRIDGE_ABI_VERSION 1u
#define RSF_UNITY_NATIVE_ABI_VERSION 2u
#define RSF_UNITY_RENDER_EVENT 0x52534601

// Copied at graph execution. Textures are Unity ID3D12Resource pointers borrowed until enqueue
// copies this record and acquires COM leases. A successful enqueue returns an opaque event-data
// pointer that must be passed exactly once to the matching plugin event; never free/dereference it.
// flags: 1 history reset, 2 probe/no SR, 4 window/overlay, 8 resolve current swap-chain buffer,
// 16 spatial-only fallback, 32 depth/motion FG input observation. Combinations select the route.
// frame_id is Time.frameCount+1; view_key is the camera instance ID widened from uint32.
typedef struct rsf_unity_packet {
    uint32_t struct_size, abi_version;
    uint64_t session_id, frame_id, view_key;
    uint32_t generation, flags; // Policy generation rejects stale queued packets after settings changes.
    void* color;
    void* depth;
    void* motion;
    void* output;
    rsf_camera_frame camera; // Jitter in render pixels, FOV in radians, delta_time in seconds.
    float previous_clip_to_clip[16];
} rsf_unity_packet;

// Main-thread configuration snapshot. Input/output extents are pixels; enabled is zero while
// native admission, output dimensions or a draining policy change disallow reconstruction.
typedef struct rsf_unity_config {
    uint32_t struct_size, abi_version, enabled, backend, quality, generation;
    uint32_t render_width, render_height, output_width, output_height;
} rsf_unity_config;

// Copied by managed Bootstrap.Start. Native function pointers and their module remain valid until
// producers/queued events/GPU work drain and the lifecycle stops. All calls use the native C ABI.
typedef struct rsf_unity_native_api {
    uint32_t struct_size, abi_version;
    uint64_t session_id;
    void (*log)(const char* message); // Synchronous, borrowed terminated message.
    int (*config)(uint32_t output_width, uint32_t output_height, rsf_unity_config* output); // 1 = populated.
    void* (*enqueue)(const rsf_unity_packet* packet); // Null on inactive/invalid/full queue.
    void* render_event; // UnityRenderingEventAndData callback; event IDs come from native exports.
    void (*managed_state)(uint32_t stage); // 0 unset, 1 awaiting loop, 2 installed, 3 refused, 4 stopped.
    void (*cpu_event)(uint32_t stage, uint64_t frame_id); // rsf_game_cpu_event stage and shared source ID.
} rsf_unity_native_api;
