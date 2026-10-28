/* SPDX-License-Identifier: MIT */
#ifndef RSF_GAME_RENDERER_H
#define RSF_GAME_RENDERER_H
#include <stdint.h>
#include <rescaleframe/game_frame.h>
#ifdef __cplusplus
extern "C" {
#endif

/* Copied engine rendering identity. Keys identify owners and must not be dereferenced by a host.
   native_frame is not an input/simulation frame ID and does not authorize frame generation. */
typedef struct rsf_game_render_pass {
    uint32_t struct_size;
    uint32_t role;
    uint64_t session_id;
    uint64_t family_key;
    uint64_t view_key;
    uint64_t pass_key;
    uint64_t native_frame;
    uint64_t scope_id;
    uint32_t resource_generation;
    uint32_t flags;
    int32_t render_rect[4];
    int32_t output_rect[4];
    /* Native graphics resources leased by the plugin until the matching end callback. */
    void* color_input;
    void* color_output;
    void* depth;
    void* motion;
    void* exposure;
    rsf_camera_frame camera;
    float previous_clip_to_clip[16];
    uint32_t camera_valid;
    uint64_t history_key;
    void* color_output_readable;
    /* Decoded game motion -> current-minus-previous UV, Y down. Sparse coverage may still need
       camera reconstruction even when written samples contain camera motion. */
    float motion_to_uv[2];
    uint32_t motion_camera_included;
    /* Raw engine-owned UI raster consumed by this native composition pass. Its alpha/effects
       conventions are game-specific; this alone does not promise a compositable FG UI layer. */
    void* ui_input;
    /* Bound during renderer construction inside the engine input frame, copied before renderer
       retirement. Zero means unassociated. Neither field establishes final Present ownership. */
    uint64_t source_frame_id;
    uint64_t submission_id;
    /* Engine game-viewport Draw ownership copied at renderer construction. Opaque and never
       dereferenced by the host; it does not identify the final Slate window or DXGI swapchain. */
    uint64_t viewport_key;
    uint64_t window_key;
    uint64_t rhi_viewport_key;
    /* Native DXGI swapchain leased through matching execution end. WINDOW scopes carry
       queued window ownership; matching actual Present still needs an execution-thread check. */
    void* swapchain;
    /* Actual family render-target surface leased through execution end, distinct from intermediate
       postprocess pools. FINAL_SCENE completion precedes Slate window composition. */
    void* scene_surface;
    /* Leased native texture supplied to a mapped ordinary Slate pixel-shader binding. */
    void* sampled_texture;
    uint32_t texture_slot;
    /* CPU-owned classification copied with the renderer. Unknown suspends interpolation. */
    uint32_t screen;
    /* TRANSLUCENT_MESH scopes surround the native material draw commands. Keys are copied
       engine identities, never dereferenced by the host. The opacity clip value is the material's
       own setting; blend mode follows the plugin convention (AC7: translucent=2, additive=3,
       modulate=4, alpha-composite=5). Native colour drawing remains unchanged. */
    uint64_t primitive_key;
    uint64_t vertex_factory_key;
    uint64_t material_key;
    uint64_t mesh_element_mask;
    uint32_t material_blend;
    float material_opacity_clip;
    /* Native mesh-element upper bound, in generated vertices before GS/tessellation expansion.
       Supplies storage for GPU-written indirect arguments without a CPU/GPU readback. */
    uint32_t geometry_capacity;
    /* Optional engine-owned frame context, leased only during this callback.
       Its interpretation is fixed by the adapter's host contract. Native-SDK
       adapters use it to share the engine's existing token, never minting a
       second identity for the same frame. It is not a retained resource lease. */
    void* native_frame_context;
    /* Adapter-selected material texture sampling bias. The runtime applies
       this value; engine-specific derivation and clamps belong to the adapter. */
    float material_texture_mip_bias;
} rsf_game_render_pass;

#define RSF_GAME_RENDER_SR 1u
#define RSF_GAME_RENDER_UI_COMPOSITE 5u
#define RSF_GAME_RENDER_WINDOW 9u
#define RSF_GAME_RENDER_FINAL_SCENE 10u
#define RSF_GAME_RENDER_TEXTURE_BINDING 11u
#define RSF_GAME_RENDER_SUBMISSION 12u
/* Entire native RHI frame, including Slate and Present. No scene view is required. */
#define RSF_GAME_RENDER_FRAME 13u
/* One translucency pass of a primary scene view. color_input is scene colour, leased through end
   and resolved again there. Begin precedes every translucent draw of the pass, so scene colour at
   begin is the opaque-only image. With TRANSLUCENCY_LAYER the pass draws an offscreen layer
   instead, returned in color_output at end; it composites as scene * alpha + rgb. render_rect
   is the view rectangle in scene colour. Identity matches the view's later SR pass. */
#define RSF_GAME_RENDER_TRANSLUCENCY 14u
/* Volumetric cloud depth for motion, delivered inline on the graphics context when the clouds
   composite. depth is an R32_FLOAT device-depth texture in the scene's convention, zero where no
   cloud qualifies, borrowed for the callback. render_rect is the view rectangle it covers. The
   host reprojects pixels without object velocity at the nearer of this and scene depth. */
#define RSF_GAME_RENDER_CLOUD_DEPTH 15u
/* The material draws of a primary scene view rendered for reconstruction (the base pass).
   render_rect and output_rect give the reconstruction ratio. Material textures sampled inside
   this scope and TRANSLUCENCY scopes are resolved to output resolution afterwards. */
#define RSF_GAME_RENDER_MATERIALS 16u
/* Normalize depth/motion for generation without reconstructing scene colour. */
#define RSF_GAME_RENDER_FG_INPUTS 17u
/* Per-mesh geometry input production inside a primary TRANSLUCENCY pass. Includes copied
   material/primitive identity, camera, rectangles and native frame. All GPU work executes on
   the enclosing graphics stream; CPU mesh pointers are not borrowed into that stream. */
#define RSF_GAME_RENDER_TRANSLUCENT_MESH 18u
/* Queued before native post-processing. motion/depth are engine-owned leased targets; the
   completed translucent velocity/depth producer commits them before temporal/DoF consumers. */
#define RSF_GAME_RENDER_VELOCITY_FINALIZE 19u
#define RSF_GAME_RENDER_RESET 1u
#define RSF_GAME_RENDER_PRIMARY 2u
/* Constructor followed the engine update-to-redraw boundary. This does not promise vendor
   marker submission or final Present ownership. */
#define RSF_GAME_RENDER_AFTER_SIMULATION 4u
/* TRANSLUCENCY scopes only: the pass renders an offscreen layer rather than into scene colour. */
#define RSF_GAME_RENDER_TRANSLUCENCY_LAYER 8u
/* This pass has usable SR inputs but cannot establish generation eligibility. */
#define RSF_GAME_RENDER_NO_FG 16u
/* The render or output configuration is changing; the pass carries no usable frame. Jitter lives in
   camera.jitter_pixels. */
#define RSF_GAME_RENDER_RECONFIGURE 0x80000000u
typedef struct rsf_game_render_config {
    uint32_t struct_size;
    uint32_t enabled;
    uint32_t output_width;
    uint32_t output_height;
    uint32_t render_width;
    uint32_t render_height;
} rsf_game_render_config;
/* Read on the engine producer thread. The host publishes a consistent settings snapshot. */
typedef int (*rsf_game_render_config_fn)(void* user, rsf_game_render_config* config);

typedef void (*rsf_game_log_fn)(void* user, const char* message);
/* Called on the graphics execution stream, before/after the engine pass's commands. The pass
   is borrowed only for the callback. The host restores its graphics state before returning. */
typedef void (*rsf_game_render_pass_fn)(void* user, void* native_command_list,
                                       const rsf_game_render_pass* pass, uint32_t begin);
#define RSF_GAME_CPU_FRAME_BEGIN 0u
#define RSF_GAME_CPU_INPUT_SAMPLE 1u
#define RSF_GAME_CPU_SIMULATION_BEGIN 2u
#define RSF_GAME_CPU_SIMULATION_END 3u
#define RSF_GAME_CPU_FRAME_END 4u
#define RSF_GAME_CPU_INPUT_EVENT 5u
/* After native BeginFrame was queued, before the engine's time update and input handling. */
#define RSF_GAME_CPU_PACING 6u
#define RSF_GAME_INPUT_KEYBOARD 1u
#define RSF_GAME_INPUT_MOUSE 2u
#define RSF_GAME_INPUT_CONTROLLER 4u
typedef struct rsf_game_cpu_event {
    uint32_t struct_size;
    uint32_t stage;
    uint64_t session_id;
    uint64_t source_frame_id;
    uint64_t timestamp_qpc;
    uint64_t qpc_frequency;
    /* At FRAME_END, number of primary renderers submitted with this source identity.
       This is an ownership fact, not a request to add a CPU or GPU wait. */
    uint32_t render_expected;
    /* INPUT_EVENT only. Zero kind with a registered Windows message is a latency-tool ping
       candidate. The runtime validates its SDK message ID. Never contains key/button values. */
    uint32_t input_kind;
    uint32_t message_id;
} rsf_game_cpu_event;
/* Runs at the actual native CPU boundary. Borrowed only during the call; no graphics-context
   work may be performed here. FRAME_END closes CPU ownership, not GPU or Present ownership. */
typedef void (*rsf_game_cpu_event_fn)(void* user, const rsf_game_cpu_event* event);

/* Synchronous replacement of an engine upscale. Zero accepts the replacement only after the
   host has recorded a complete output. Any other result leaves native evaluation enabled.
   Inputs are NON_PIXEL_SHADER_RESOURCE and output is COPY_DEST during this callback. The
   plugin restores their engine states; the enclosing engine wrapper restores pipeline state.
   Even a refused evaluation may record GPU work and therefore needs submission retirement. */
typedef int32_t (*rsf_game_process_upscale_fn)(void* user, void* native_command_list,
                                              const rsf_game_render_pass* pass);
/* The actual ExecuteCommandLists call, before (begin=1) and after (begin=0). Lists are borrowed
   during this callback and cannot be retained as recording identities after engine recycling.
   submission_id pairs these two callbacks, including nested submissions. */
typedef void (*rsf_game_submit_fn)(void* user, void* native_queue, uint64_t submission_id,
                                 uint32_t count, void* const* native_lists, uint32_t begin);
/* Actual Present before/after, preserving the native swapchain and overlay call chain. The
   callback observes bookkeeping boundaries; it cannot replace the engine's Present result. */
typedef void (*rsf_game_present_fn)(void* user, void* native_swapchain, uint32_t sync_interval,
                                  uint32_t flags, uint32_t begin);
/* Native engine latency boundaries. Markers 0..6 follow game_frame.h; SLEEP precedes input.
   MODE reports an attempted native pacing-policy write, which an active host may replace.
   A zero callback result means the host owns this operation, including reporting its errors;
   the plugin must not perform a second native sleep or duplicate the marker/mode write. */
#define RSF_GAME_LATENCY_SLEEP 0x100u
#define RSF_GAME_LATENCY_MODE 0x101u
typedef struct rsf_game_latency_event {
    uint32_t struct_size;
    uint32_t marker;
    uint64_t session_id;
    uint64_t source_frame_id;
    uint64_t timestamp_qpc;
    uint64_t qpc_frequency;
} rsf_game_latency_event;
typedef int32_t (*rsf_game_process_latency_fn)(void* user, const rsf_game_latency_event* event);

/* Current renderer admission, reported even when no usable pass can be published. */
#define RSF_GAME_RENDER_FEEDBACK_ACCEPTED 0u
#define RSF_GAME_RENDER_FEEDBACK_INPUT_UNSUPPORTED 1u
#define RSF_GAME_RENDER_FEEDBACK_SUBRECT_UNSUPPORTED 2u
#define RSF_GAME_RENDER_FEEDBACK_CAMERA_UNAVAILABLE 3u
#define RSF_GAME_RENDER_FEEDBACK_OUTPUT_UNSUPPORTED 4u
#define RSF_GAME_RENDER_FEEDBACK_CAMERA_JITTER_INVALID 5u
#define RSF_GAME_RENDER_FEEDBACK_PRESENT_UNAVAILABLE 6u
#define RSF_GAME_RENDER_FEEDBACK_HOST_REFUSED 7u
typedef struct rsf_game_render_feedback {
    uint32_t struct_size, reason;
    uint64_t session_id, native_frame;
} rsf_game_render_feedback;
typedef void (*rsf_game_render_feedback_fn)(void* user, const rsf_game_render_feedback* feedback);

typedef struct rsf_game_host_services {
    uint32_t struct_size;
    uint32_t abi_version;
    uint64_t session_id;
    rsf_game_log_fn log;
    rsf_game_render_pass_fn render_pass;
    void* user;
    rsf_game_render_config_fn render_config;
    rsf_game_cpu_event_fn cpu_event;
    /* Optional current-scope observer for diagnostics. At end it receives the restored parent
       (or a zero scope). render_pass still receives the pass that actually completed. */
    rsf_game_render_pass_fn scope_state;
    rsf_game_process_upscale_fn process_upscale;
    rsf_game_submit_fn submit;
    rsf_game_present_fn present;
    rsf_game_process_latency_fn process_latency;
    rsf_game_render_feedback_fn render_feedback;
} rsf_game_host_services;
typedef struct rsf_game_prepare_args {
    uint32_t struct_size;
    uint32_t abi_version;
    const rsf_game_host_services* host;
} rsf_game_prepare_args;
typedef struct rsf_game_start_args {
    uint32_t struct_size;
    uint32_t abi_version;
} rsf_game_start_args;
typedef struct rsf_game_control_args {
    uint32_t struct_size;
    uint32_t abi_version;
} rsf_game_control_args;
typedef struct rsf_game_renderer_status {
    uint32_t struct_size;
    uint32_t abi_version;
    uint32_t prepared;
    uint32_t active;
    uint32_t rendering_ready;
    const char* reason;
} rsf_game_renderer_status;
#ifdef __cplusplus
}
#endif
#endif
