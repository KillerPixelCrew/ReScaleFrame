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
    float jitter_pixels[2];
    float previous_jitter_pixels[2];
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
} rsf_game_render_pass;

#define RSF_GAME_RENDER_SR 1u
#define RSF_GAME_RENDER_UI_COMPOSITE 5u
#define RSF_GAME_RENDER_WINDOW 9u
#define RSF_GAME_RENDER_FINAL_SCENE 10u
#define RSF_GAME_RENDER_TEXTURE_BINDING 11u
#define RSF_GAME_RENDER_RESET 1u
#define RSF_GAME_RENDER_PRIMARY 2u
/* Constructor followed the engine update-to-redraw boundary. This does not promise vendor
   marker submission or final Present ownership. */
#define RSF_GAME_RENDER_AFTER_SIMULATION 4u
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
typedef struct rsf_game_cpu_event {
    uint32_t struct_size;
    uint32_t stage;
    uint64_t session_id;
    uint64_t source_frame_id;
    uint64_t timestamp_qpc;
    uint64_t qpc_frequency;
} rsf_game_cpu_event;
/* Runs at the actual native CPU boundary. Borrowed only during the call; no graphics-context
   work may be performed here. FRAME_END closes CPU ownership, not GPU or Present ownership. */
typedef void (*rsf_game_cpu_event_fn)(void* user, const rsf_game_cpu_event* event);

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
