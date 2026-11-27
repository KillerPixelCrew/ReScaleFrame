/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal to the Rust frame tap. Defines the hook records and callback boundary. */
#include <rescaleframe/frame_tap.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Hook ids in install order. Refresh, rollback and uninstall walk them in this order. */
enum {
    RSF_FRAME_TAP_HOOK_VERTEX_BUFFERS,
    RSF_FRAME_TAP_HOOK_INDEX_BUFFER,
    RSF_FRAME_TAP_HOOK_LAYOUT,
    RSF_FRAME_TAP_HOOK_TOPOLOGY,
    RSF_FRAME_TAP_HOOK_VERTEX_SHADER,
    RSF_FRAME_TAP_HOOK_VERTEX_CONSTANTS,
    RSF_FRAME_TAP_HOOK_DRAW_INDEXED_INSTANCED,
    RSF_FRAME_TAP_HOOK_DRAW_INSTANCED,
    RSF_FRAME_TAP_HOOK_CLEAR_STATE,
    RSF_FRAME_TAP_HOOK_EXECUTE,
    RSF_FRAME_TAP_HOOK_DRAW_INDEXED_INSTANCED_INDIRECT,
    RSF_FRAME_TAP_HOOK_DRAW_INSTANCED_INDIRECT,
    RSF_FRAME_TAP_HOOK_DRAW_AUTO,
    RSF_FRAME_TAP_HOOK_PIXEL_SHADER,
    RSF_FRAME_TAP_HOOK_BLEND,
    RSF_FRAME_TAP_HOOK_DEPTH_STENCIL,
    RSF_FRAME_TAP_HOOK_VIEWS,
    RSF_FRAME_TAP_HOOK_PIXEL_CONSTANTS,
    RSF_FRAME_TAP_HOOK_SAMPLERS,
    RSF_FRAME_TAP_HOOK_DRAW_INDEXED,
    RSF_FRAME_TAP_HOOK_DRAW,
    RSF_FRAME_TAP_HOOK_TARGETS,
    RSF_FRAME_TAP_HOOK_TARGETS_AND_UAVS,
    RSF_FRAME_TAP_HOOK_VIEWPORTS,
    RSF_FRAME_TAP_HOOK_SCISSORS,
    RSF_FRAME_TAP_HOOK_CLEAR_TARGET,
    RSF_FRAME_TAP_HOOK_MAP,
    RSF_FRAME_TAP_HOOK_UNMAP,
    RSF_FRAME_TAP_HOOK_FLUSH,
    RSF_FRAME_TAP_HOOK_DISPATCH,
    RSF_FRAME_TAP_HOOK_DISPATCH_INDIRECT,
    RSF_FRAME_TAP_HOOK_COPY_REGION,
    RSF_FRAME_TAP_HOOK_COPY,
    RSF_FRAME_TAP_HOOK_UPDATE,
    RSF_FRAME_TAP_HOOK_COPY_STRUCTURE_COUNT,
    RSF_FRAME_TAP_HOOK_CLEAR_UAV_UINT,
    RSF_FRAME_TAP_HOOK_CLEAR_UAV_FLOAT,
    RSF_FRAME_TAP_HOOK_CLEAR_DEPTH,
    RSF_FRAME_TAP_HOOK_GENERATE_MIPS,
    RSF_FRAME_TAP_HOOK_RESOLVE,
    RSF_FRAME_TAP_HOOK_COPY_REGION1,
    RSF_FRAME_TAP_HOOK_UPDATE1,
    RSF_FRAME_TAP_HOOK_DISCARD_RESOURCE,
    RSF_FRAME_TAP_HOOK_COUNT
};

/* Pipeline objects whose identity the shadow records. */
enum {
    RSF_FRAME_TAP_OBJECT_LAYOUT,
    RSF_FRAME_TAP_OBJECT_VERTEX_SHADER,
    RSF_FRAME_TAP_OBJECT_PIXEL_SHADER,
    RSF_FRAME_TAP_OBJECT_BLEND,
    RSF_FRAME_TAP_OBJECT_DEPTH_STENCIL
};

/* What the shadow records of a bound texture. */
typedef struct rsf_texture_facts {
    uint32_t struct_size;
    uint32_t width;
    uint32_t height;
    uint32_t format;
    uint32_t bind_flags;
    uint32_t mip_levels;
    uint32_t array_size;
    uint32_t sample_count;
} rsf_texture_facts;

/* One vtable slot, counting the three IUnknown and four ID3D11DeviceChild entries. `context1`
   slots exist only on ID3D11DeviceContext1. */
typedef struct rsf_frame_tap_hook {
    uint32_t slot;
    uint32_t context1;
    void* replacement;
} rsf_frame_tap_hook;

/* The unordered access arguments of OMSetRenderTargetsAndUnorderedAccessViews. */
typedef struct rsf_frame_tap_uavs {
    uint32_t start;
    uint32_t count;
    void* const* views;
    const uint32_t* initial_counts;
} rsf_frame_tap_uavs;

/* kind uses rsf_frame_tap_geometry numbering: 0 Draw, 1 DrawIndexed, 2 DrawInstanced,
   3 DrawIndexedInstanced. */
typedef struct rsf_frame_tap_draw_call {
    uint32_t kind;
    uint32_t count;
    uint32_t start;
    uint32_t instances;
    uint32_t first_instance;
    int32_t base_vertex;
} rsf_frame_tap_draw_call;

/* Callers' callbacks. Foreign exceptions stay on the native side. */
void rsf_frame_tap_call_report(rsf_frame_tap_target_fn fn, void* user,
                               const rsf_frame_tap_target_draw* draw);
void rsf_frame_tap_call_geometry(rsf_frame_tap_geometry_fn fn, void* user,
                                 const rsf_frame_tap_geometry* geometry);
void rsf_frame_tap_call_compute(rsf_frame_tap_compute_fn fn, void* user, void* context,
                                uint32_t x, uint32_t y, uint32_t z, void* arguments,
                                uint32_t offset);

void rsf_frame_tap_call_constant_observer(
    rsf_frame_tap_constant_observer_fn fn, void* user, void* buffer,
    const void* contents, uint32_t bytes);

#ifdef __cplusplus
}
#endif
