// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <stdint.h>
#include <stddef.h>
#include <rescaleframe/game_frame.h>

#define RSF_FSR4FG_BRIDGE_ABI_VERSION 1u
#define RSF_FSR4FG_INVALID_ARGUMENT (-13001)
#define RSF_FSR4FG_ABI_MISMATCH (-13002)
#define RSF_FSR4FG_NOT_SUPPORTED (-13003)
typedef struct rsf_fsr4fg_bridge rsf_fsr4fg_bridge;
typedef struct rsf_fsr4fg_create {
    uint32_t struct_size, abi_version;
    uint64_t adapter_luid;
    const uint16_t* runtime_directory; /* Absolute UTF-16, borrowed during create. */
    uint32_t width, height, depth_inverted, depth_infinite, motion_jittered, generated_count;
} rsf_fsr4fg_create;
typedef struct rsf_fsr4fg_info {
    uint32_t struct_size, abi_version;
    uint64_t adapter_luid, provider_version;
    uint32_t vendor_id, max_generated;
} rsf_fsr4fg_info;
typedef struct rsf_fsr4fg_frame {
    uint32_t struct_size, abi_version;
    const rsf_frame_record* record; /* Synchronous borrow, camera ABI 1. */
    void* color; void* depth; void* motion; void* hudless; /* ID3D12Resource*, COMMON. */
    float motion_scale_x, motion_scale_y; /* Stored motion -> render pixels. */
    uint64_t ready_value, done_value;
    uint32_t enabled, vsync;
} rsf_fsr4fg_frame;
typedef struct rsf_fsr4fg_present_status {
    uint32_t struct_size, abi_version;
    uint32_t frames_presented, fg_enabled;
    int32_t result;
    uint32_t terminal; /* Unknown GPU completion: preserve every native owner. */
    uint64_t generated_callbacks;
} rsf_fsr4fg_present_status;
static inline int32_t rsf_fsr4fg_check(uint32_t size, uint32_t version, size_t required) {
    if (size < required) return RSF_FSR4FG_INVALID_ARGUMENT;
    return version == RSF_FSR4FG_BRIDGE_ABI_VERSION ? 0 : RSF_FSR4FG_ABI_MISMATCH;
}
#ifdef __cplusplus
extern "C" {
#endif
/* Exact family-4 discovery AND context verification happen before any HWND call.
   A failed checked cleanup may retain *output; the caller must not discard it. */
int32_t RsfFsr4FgCreate(const rsf_fsr4fg_create*, rsf_fsr4fg_bridge**);
/* Enumeration only: matching DX12 device and DLL, no SDK context, queues or HWND. */
int32_t RsfFsr4FgProbe(uint64_t adapter_luid, const uint16_t* directory, rsf_fsr4fg_info*);
int32_t RsfFsr4FgInfo(const rsf_fsr4fg_bridge*, rsf_fsr4fg_info*);
int32_t RsfFsr4FgStart(rsf_fsr4fg_bridge*, void* window);
int32_t RsfFsr4FgCreateImage(void*, uint32_t width, uint32_t height, uint32_t format, void** handle, void** resource);
void RsfFsr4FgReleaseImage(void* resource);
int32_t RsfFsr4FgCreateFence(void*, void** handle);
int32_t RsfFsr4FgPresent(rsf_fsr4fg_bridge*, const rsf_fsr4fg_frame*, rsf_fsr4fg_present_status*);
int32_t RsfFsr4FgDrain(rsf_fsr4fg_bridge*);
int32_t RsfFsr4FgPrepareDestroy(rsf_fsr4fg_bridge*);
int32_t RsfFsr4FgDestroy(rsf_fsr4fg_bridge*);
#ifdef __cplusplus
}
#endif
