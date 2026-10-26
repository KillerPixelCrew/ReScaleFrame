// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <stdint.h>
#include <stddef.h>
#define RSF_FSR4_BRIDGE_ABI_VERSION 1u
#define RSF_FSR4_BRIDGE_INVALID_ARGUMENT (-11001)
#define RSF_FSR4_BRIDGE_ABI_MISMATCH (-11002)
#if defined(RSF_FSR4_BRIDGE_BUILD)
#define RSF_FSR4_BRIDGE_API __declspec(dllexport)
#else
#define RSF_FSR4_BRIDGE_API
#endif
typedef struct VulkanStoryFsr4 rsf_fsr4_bridge;
typedef struct rsf_fsr4_bridge_create {
    uint32_t struct_size, abi_version;
    uint64_t adapter_luid;
    const uint16_t* runtime_directory; /* Borrowed absolute UTF-16 directory. */
    uint32_t render_width, render_height, output_width, output_height;
    uint32_t depth_inverted, depth_infinite, motion_jittered, reserved;
} rsf_fsr4_bridge_create;
typedef struct rsf_fsr4_bridge_frame {
    uint32_t struct_size, abi_version;
    void* color; void* depth; void* motion; void* output; /* ID3D12Resource*, COMMON. */
    uint32_t render_width, render_height, output_width, output_height;
    float jitter_x, jitter_y, motion_scale_x, motion_scale_y;
    float delta_ms, near_plane, far_plane, vertical_fov;
    float pre_exposure;
    uint32_t reset;
    uint64_t ready_value, done_value; /* One increasing bidirectional timeline. */
} rsf_fsr4_bridge_frame;
typedef struct rsf_fsr4_bridge_info {
    uint32_t struct_size, abi_version;
    uint64_t adapter_luid, provider_version;
    uint32_t vendor_id, int8_compatibility;
} rsf_fsr4_bridge_info;
static inline int32_t rsf_fsr4_bridge_check(uint32_t size, uint32_t version, size_t required) {
    if (size < required) return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    return version == RSF_FSR4_BRIDGE_ABI_VERSION ? 0 : RSF_FSR4_BRIDGE_ABI_MISMATCH;
}
#ifdef __cplusplus
extern "C" {
#endif
/* NVIDIA/Intel admission requires the pinned signed INT8 compatibility lease.
   A provider mismatch refuses creation. A failed cleanup may retain *output. */
RSF_FSR4_BRIDGE_API int32_t RsfFsr4BridgeCreate(const rsf_fsr4_bridge_create*, rsf_fsr4_bridge**);
RSF_FSR4_BRIDGE_API int32_t RsfFsr4BridgeInfo(const rsf_fsr4_bridge*, rsf_fsr4_bridge_info*);
RSF_FSR4_BRIDGE_API int32_t RsfFsr4BridgePlan(rsf_fsr4_bridge*, uint32_t quality, uint32_t*, uint32_t*);
/* Valid ready/done values always receive an ordered handoff, including SDK
   failures. A failed output must not be copied into the engine image. */
RSF_FSR4_BRIDGE_API int32_t RsfFsr4BridgeEvaluate(rsf_fsr4_bridge*, const rsf_fsr4_bridge_frame*);
#ifdef __cplusplus
}
#endif
