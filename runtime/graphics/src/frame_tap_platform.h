/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal to rsf_graphics: the native leaves the Rust frame tap owner calls, and its entries.
   Rust owns hook admission and retirement, plan and callback state, gates, substitution,
   classification, capture routing and the references the tap holds. The typed COM detours and
   every D3D11 call stay here. */
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

/* One vtable slot, counting the three IUnknown and four ID3D11DeviceChild entries. `context1`
   slots exist only on ID3D11DeviceContext1. */
typedef struct rsf_frame_tap_hook {
    uint32_t slot;
    uint32_t context1;
    void* replacement;
} rsf_frame_tap_hook;

/* D3D11_VIEWPORT and D3D11_RECT. */
typedef struct rsf_frame_tap_viewport {
    float x, y, width, height, min_depth, max_depth;
} rsf_frame_tap_viewport;

typedef struct rsf_frame_tap_rect {
    int32_t left, top, right, bottom;
} rsf_frame_tap_rect;

/* Output merger and rasterizer state a divert saves. Every object is an owned reference. */
typedef struct rsf_frame_tap_output {
    void* targets[8];
    void* depth_view;
    void* blend;
    float blend_factor[4];
    uint32_t blend_mask;
    uint32_t viewport_count;
    rsf_frame_tap_viewport viewports[16];
    uint32_t scissor_count;
    rsf_frame_tap_rect scissors[16];
} rsf_frame_tap_output;

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

/* Render target zero of the game's blend before the alpha patch, and the patched write mask. */
typedef struct rsf_frame_tap_blend_log {
    uint32_t enable;
    uint32_t source;
    uint32_t destination;
    uint32_t operation;
    uint32_t alpha_source;
    uint32_t alpha_destination;
    uint32_t alpha_operation;
    uint32_t write_mask;
    uint32_t patched_write_mask;
} rsf_frame_tap_blend_log;

/* `original` is the hooked method's original. A null `original` on set_viewports and set_scissors
   calls through the context, which reaches the hooks as the tap's own work. */
typedef struct rsf_frame_tap_platform {
    int32_t (*patch)(void** vtable, uint32_t slot, void* replacement);
    int32_t (*context1)(void* context);
    void* (*texture_behind)(void* view);
    void* (*view_resource)(void* view);
    void (*texture_facts)(void* texture, rsf_texture_facts* facts);
    uint32_t (*target_view_format)(void* view);
    /* Zero when the resource is not a buffer. */
    uint32_t (*buffer_width)(void* resource, uint32_t* bind_flags);
    /* Every pixel shader resource slot, as owned references. */
    void (*shader_views)(void* context, void** views);
    void (*set_shader_views)(void* original, void* context, uint32_t start, uint32_t count,
                             void* const* views);
    void (*set_targets)(void* original, void* context, uint32_t count, void* const* views,
                        void* depth, const rsf_frame_tap_uavs* uavs);
    void (*set_viewports)(void* original, void* context, uint32_t count,
                          const rsf_frame_tap_viewport* viewports);
    void (*set_scissors)(void* original, void* context, uint32_t count,
                         const rsf_frame_tap_rect* rectangles);
    uint32_t (*viewport)(void* context, rsf_frame_tap_viewport* viewport);
    void (*set_blend)(void* context, void* blend, const float* factor, uint32_t mask);
    void (*save_output)(void* context, rsf_frame_tap_output* output);
    /* An owned blend state with the alpha operations patched, or null. */
    void* (*patch_blend)(void* context, void* blend, rsf_frame_tap_blend_log* log);
    /* Borrowed geometry, hull and domain constant buffers, 14 each. */
    void (*stage_constants)(void* context, void** geometry, void** hull, void** domain);
    /* Stage 0 and 1 call `original`, the vertex or pixel constant setter. */
    void (*bind_constants)(void* original, void* context, uint32_t stage, uint32_t slot,
                           void* buffer);
    void (*seed_geometry)(void* context, rsf_frame_tap_geometry* geometry,
                          void** pixel_constants);
    void (*draw)(void* original, void* context, const rsf_frame_tap_draw_call* call);
    /* Every pixel shader sampler slot, as owned references. */
    void (*samplers)(void* context, void** samplers);
    void (*set_samplers)(void* original, void* context, uint32_t start, uint32_t count,
                         void* const* samplers);
    /* An owned biased clone, or null when the sampler is not biased or creation failed. */
    void* (*clone_sampler)(void* context, void* sampler, float bias);
    /* Callers' callbacks. Foreign exceptions stay on the native side; a throwing gate, verdict or
       override reports decline, leave and nothing. */
    void (*log)(rsf_frame_tap_log_fn fn, void* user, const char* message);
    void (*pass)(rsf_frame_tap_fn fn, void* user, const rsf_frame_tap_pass* pass);
    void (*report)(rsf_frame_tap_target_fn fn, void* user, const rsf_frame_tap_target_draw* draw);
    void (*geometry)(rsf_frame_tap_geometry_fn fn, void* user,
                     const rsf_frame_tap_geometry* geometry);
    void (*compute)(rsf_frame_tap_compute_fn fn, void* user, void* context, uint32_t x,
                    uint32_t y, uint32_t z, void* arguments, uint32_t offset);
    void (*constants)(rsf_frame_tap_constants_fn fn, void* user, void* buffer, void* contents,
                      uint32_t bytes);
    int32_t (*gate)(rsf_frame_tap_gate_fn fn, void* user, void* context, void* texture);
    rsf_frame_tap_verdict (*verdict)(rsf_frame_tap_verdict_fn fn, void* user,
                                     const rsf_frame_tap_target_draw* draw);
    int32_t (*override_constants)(rsf_frame_tap_constant_override_fn fn, void* user,
                                  const rsf_frame_tap_target_draw* draw, uint32_t* slots,
                                  void** buffers);
} rsf_frame_tap_platform;

typedef const rsf_frame_tap_platform* rsf_frame_tap_platform_ref;

/* Control entries. */
rsf_frame_tap_result rsf_core_frame_tap_install(rsf_frame_tap_platform_ref, void* context,
                                                const rsf_frame_tap_options* options,
                                                const rsf_frame_tap_hook* hooks);
rsf_frame_tap_result rsf_core_frame_tap_uninstall(rsf_frame_tap_platform_ref);
rsf_frame_tap_result rsf_core_frame_tap_refresh(rsf_frame_tap_platform_ref);
rsf_frame_tap_result rsf_core_frame_tap_set_sampler_bias(rsf_frame_tap_platform_ref, float bias);
rsf_frame_tap_result rsf_core_frame_tap_set_plan(const rsf_frame_tap_plan* plan);
rsf_frame_tap_result rsf_core_frame_tap_end_frame(void);
rsf_frame_tap_result rsf_core_frame_tap_watch_input(void* texture);
rsf_frame_tap_result rsf_core_frame_tap_set_constant_watch(uint32_t bytes,
                                                           rsf_frame_tap_constants_fn fn,
                                                           void* user);
void rsf_core_frame_tap_set_constant_watch_writable(uint32_t writable);
rsf_frame_tap_result rsf_core_frame_tap_set_research(rsf_frame_tap_target_fn before,
                                                     rsf_frame_tap_target_fn after,
                                                     rsf_frame_tap_compute_fn compute,
                                                     void* user);
void* rsf_core_frame_tap_bound_target(void);
rsf_frame_tap_result rsf_core_frame_tap_set_constant_override(
    rsf_frame_tap_constant_override_fn fn, void* user);
void rsf_core_frame_tap_set_constant_override_format(uint32_t format);
rsf_frame_tap_result rsf_core_frame_tap_set_divert(rsf_frame_tap_platform_ref,
                                                   const rsf_frame_tap_divert_setup* setup);
rsf_frame_tap_result rsf_core_frame_tap_set_candidates(const rsf_frame_tap_candidates* candidates);
rsf_frame_tap_result rsf_core_frame_tap_watch_target(uint32_t index, void* texture,
                                                     uint32_t limit);
rsf_frame_tap_result rsf_core_frame_tap_get_status(rsf_frame_tap_status* status);

/* Hook entries. `enter` re-applies a rewritten vtable and returns the hook's current original;
   `leave` re-applies it after a call that may have rewritten it.

   A hook that reads the plan or the render thread's shadow holds an admission, and set_plan and
   uninstall wait until only their own thread holds any. Entries whose decision the detour
   forwards (clear, copy, samplers) set `*admitted` when they hold one, and the detour
   passes it to `leave` after forwarding, so a substitute or clone cannot outlive a clear. */
void* rsf_core_frame_tap_enter(rsf_frame_tap_platform_ref, uint32_t hook);
void rsf_core_frame_tap_leave(rsf_frame_tap_platform_ref, uint32_t admitted);
void rsf_core_frame_tap_views(rsf_frame_tap_platform_ref, void* context, uint32_t start,
                              uint32_t count, void* const* views);
void rsf_core_frame_tap_targets(rsf_frame_tap_platform_ref, void* context, uint32_t count,
                                void* const* views, void* depth, const rsf_frame_tap_uavs* uavs);
void rsf_core_frame_tap_viewports(rsf_frame_tap_platform_ref, void* context, uint32_t count,
                                  const rsf_frame_tap_viewport* viewports);
void rsf_core_frame_tap_scissors(rsf_frame_tap_platform_ref, void* context, uint32_t count,
                                 const rsf_frame_tap_rect* rectangles);
void* rsf_core_frame_tap_clear_target(rsf_frame_tap_platform_ref, void* context, void* view,
                                      uint32_t* admitted);
void rsf_core_frame_tap_draw(rsf_frame_tap_platform_ref, void* context, uint32_t hook,
                             const rsf_frame_tap_draw_call* call);
void rsf_core_frame_tap_indirect(rsf_frame_tap_platform_ref, void* context, uint32_t kind,
                                 void* arguments, uint32_t offset);
void rsf_core_frame_tap_vertex_buffers(void* context, uint32_t start, uint32_t count,
                                       void* const* buffers, const uint32_t* strides,
                                       const uint32_t* offsets);
void rsf_core_frame_tap_index_buffer(void* context, void* buffer, uint32_t format,
                                     uint32_t offset);
void rsf_core_frame_tap_object(void* context, uint32_t object, void* value);
void rsf_core_frame_tap_topology(void* context, uint32_t topology);
void rsf_core_frame_tap_vertex_constants(void* context, uint32_t start, uint32_t count,
                                         void* const* buffers);
void rsf_core_frame_tap_pixel_constants(rsf_frame_tap_platform_ref, void* context,
                                        uint32_t start, uint32_t count, void* const* buffers);
void rsf_core_frame_tap_invalidate(void* context, uint32_t restore);
void rsf_core_frame_tap_mapped(rsf_frame_tap_platform_ref, void* context, void* resource,
                               uint32_t subresource, uint32_t kind, void* data);
void rsf_core_frame_tap_unmapping(rsf_frame_tap_platform_ref, void* context, void* resource);
void rsf_core_frame_tap_dispatch(rsf_frame_tap_platform_ref, void* context, uint32_t x,
                                 uint32_t y, uint32_t z, void* arguments, uint32_t offset);
void rsf_core_frame_tap_copy(rsf_frame_tap_platform_ref, void* context, void** destination,
                             void** source, uint32_t* admitted);
const void* rsf_core_frame_tap_update(rsf_frame_tap_platform_ref, void* context,
                                      void* destination, uint32_t subresource, uint32_t has_box,
                                      const void* data, uint32_t row_pitch, void* scratch);
void* const* rsf_core_frame_tap_samplers(rsf_frame_tap_platform_ref, void* context,
                                         uint32_t start, uint32_t count, void* const* samplers,
                                         void** bound, uint32_t* admitted);

#ifdef __cplusplus
} /* extern "C" */
#endif
