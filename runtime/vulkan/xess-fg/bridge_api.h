// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <stdint.h>
#include <stddef.h>

/* Private orchestrator contract. Pointers borrow Windows/D3D12 objects; neither
   STL types nor Intel SDK structures cross this boundary. Calls are serialized.
   This is a DX12 presenter for Vulkan external resources, not a Vulkan XeSS API. */
#define RSF_XESS_BRIDGE_ABI_VERSION 1u
#define RSF_XESS_BRIDGE_INVALID_ARGUMENT (-10001)
#define RSF_XESS_BRIDGE_ABI_MISMATCH (-10002)
#if defined(RSF_XESS_BRIDGE_BUILD)
#define RSF_XESS_BRIDGE_API __declspec(dllexport)
#else
#define RSF_XESS_BRIDGE_API
#endif

typedef struct VulkanStoryXessFg rsf_xess_bridge;
typedef struct rsf_xess_bridge_create {
    uint32_t struct_size, abi_version;
    uint64_t adapter_luid;
    /* Borrowed absolute UTF-16 directory containing libxess_fg.dll/libxell.dll.
       Valid only during create; loading never searches the working directory. */
    const uint16_t* runtime_directory;
} rsf_xess_bridge_create;

typedef struct rsf_xess_bridge_start {
    uint32_t struct_size, abi_version;
    void* window;
    uint32_t width, height;
    uint32_t inverted_depth;
    uint32_t generated_count; /* Excludes the real frame; never silently clamped. */
} rsf_xess_bridge_start;

typedef struct rsf_xess_bridge_frame {
    uint32_t struct_size, abi_version;
    uint32_t frame_id, reset, vsync, reserved;
    /* Borrowed ID3D12Resource*, COMMON state, valid through GPU done. Color and
       HUD-less are output-sized BGRA8; motion is render-sized RG16F, depth R32F.
       HUD-less includes postprocessing and excludes canvas. No inferred UI alpha. */
    void* color;
    void* depth;
    void* motion;
    void* hudless;
    float view_matrix[16], projection_matrix[16]; /* Row-major, without jitter. */
    float jitter_x, jitter_y; /* Raster jitter in render pixels. */
    float motion_scale_x, motion_scale_y; /* Stored current-to-previous -> pixels. */
    /* One permanently imported D3D12 timeline fence. Every ready/done pair
       increases globally. Producer copies signal ready; reuse waits on done. */
    uint64_t ready_fence_value, done_fence_value;
} rsf_xess_bridge_frame;

typedef struct rsf_xess_bridge_present_status {
    uint32_t struct_size, abi_version;
    uint32_t frames_presented, fg_enabled;
    int32_t fg_result;
    uint32_t reserved;
} rsf_xess_bridge_present_status;

static inline int32_t rsf_xess_bridge_check(uint32_t size, uint32_t version, size_t required) {
    if (size < required) return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    return version == RSF_XESS_BRIDGE_ABI_VERSION ? 0 : RSF_XESS_BRIDGE_ABI_MISMATCH;
}

#ifdef __cplusplus
extern "C" {
#endif
/* A failed create may retain a nonnull owner when checked cleanup fails. Never
   discard it. Start requires the HWND to have no Vulkan/DXGI swapchain owner. */
RSF_XESS_BRIDGE_API int32_t RsfXessBridgeCreate(const rsf_xess_bridge_create*, rsf_xess_bridge**);
RSF_XESS_BRIDGE_API uint64_t RsfXessBridgeAdapterLuid(const rsf_xess_bridge*);
RSF_XESS_BRIDGE_API uint32_t RsfXessBridgeMaxGenerated(const rsf_xess_bridge*);
RSF_XESS_BRIDGE_API int32_t RsfXessBridgeSetGenerated(rsf_xess_bridge*, uint32_t);
RSF_XESS_BRIDGE_API int32_t RsfXessBridgeStart(rsf_xess_bridge*, const rsf_xess_bridge_start*);
RSF_XESS_BRIDGE_API int32_t RsfXessBridgePresent(rsf_xess_bridge*, const rsf_xess_bridge_frame*,
    rsf_xess_bridge_present_status*);
#ifdef __cplusplus
}
#endif
