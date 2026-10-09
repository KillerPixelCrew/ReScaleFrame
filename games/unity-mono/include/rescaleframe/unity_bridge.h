// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_api.h>

#define RSF_UNITY_BRIDGE_ABI_VERSION 2u
#define RSF_UNITY_NATIVE_ABI_VERSION 3u

// rsf_unity_packet::flags. The managed adapter mirrors these in PacketFlags (Native.cs).
// History restarts at this frame.
#define RSF_UNITY_PACKET_RESET 1u
// Camera constants only: no reconstruction is requested and depth/motion/output may be absent.
#define RSF_UNITY_PACKET_PROBE 2u
// Window event on the engine swapchain, carrying the overlay and no scene resources.
#define RSF_UNITY_PACKET_WINDOW 4u
// Completed scene colour before the UI draw. The colour is the swapchain buffer, resolved in the
// native callback because an imported back buffer has no RenderTexture to hand over.
#define RSF_UNITY_PACKET_HUDLESS 8u
// Reconstruction is the engine's spatial fallback only: URP's depth or motion cannot be used.
#define RSF_UNITY_PACKET_NO_INPUTS 16u
// Depth and motion for generation, no reconstruction.
#define RSF_UNITY_PACKET_INPUTS_ONLY 32u

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

// engine_spatial is non-zero when the engine upscales spatially itself (no jitter, no history),
// so the adapter never needs to know which vendor the host chose.
typedef struct rsf_unity_config {
    uint32_t struct_size, abi_version, enabled, engine_spatial, quality, generation;
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
    void (*cpu_event)(uint32_t stage, uint64_t frame_id);
} rsf_unity_native_api;
