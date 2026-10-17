// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/gl_shared_surface.h>
#include <rescaleframe/native_fg.h>
typedef struct rsf_gl_pipeline rsf_gl_pipeline;
typedef struct rsf_gl_pipeline_setup {
    uint32_t struct_size;
    void* hwnd;
    const char* runtime_directory_utf8;
    rsf_backend_log_fn log;
    void* user;
    const char* engine_version_utf8;
    const char* project_id_utf8;
    /* Optional borrowed child created on the GLFW window's event thread. It outlives the pipeline. */
    void* presentation_hwnd;
} rsf_gl_pipeline_setup;
typedef struct rsf_gl_pipeline_options {
    uint32_t struct_size, sr_backend, quality, fg_backend;
    rsf_native_fg_options generation;
} rsf_gl_pipeline_options;
typedef struct rsf_gl_pipeline_surfaces {
    uint32_t struct_size, render_width, render_height, output_width, output_height;
    /* GL textures with DX top-left row order. Producer blits flip vertically. */
    uint32_t scene, depth, motion, output, hudless, composed;
} rsf_gl_pipeline_surfaces;
typedef struct rsf_gl_pipeline_status {
    uint32_t struct_size, requested_sr, effective_sr, requested_fg, effective_fg;
    int32_t sr_result, fg_switch_result;
    uint64_t evaluated;
    rsf_native_fg_status generation;
} rsf_gl_pipeline_status;
#ifdef __cplusplus
extern "C" {
#endif
/* Create on the parent window's owning thread. No GL context is required.
   Destroy routes to that same thread; all DXGI/GL work remains on the graphics thread. */
RSF_RUNTIME_API void* rsf_gl_window_create(void* parent);
RSF_RUNTIME_API int rsf_gl_window_destroy(void* window);
RSF_RUNTIME_API rsf_backend_result rsf_gl_pipeline_open(const rsf_gl_pipeline_setup*, rsf_gl_pipeline**);
/* Graphics owner, drained boundary before beginning the CPU frame. No restart required. */
RSF_RUNTIME_API rsf_backend_result rsf_gl_pipeline_configure(rsf_gl_pipeline*, const rsf_gl_pipeline_options*, uint32_t width, uint32_t height);
RSF_RUNTIME_API rsf_backend_result rsf_gl_pipeline_get_surfaces(rsf_gl_pipeline*, rsf_gl_pipeline_surfaces*);
RSF_RUNTIME_API void rsf_gl_pipeline_cpu(rsf_gl_pipeline*, uint64_t frame, uint32_t stage);
/* Input consumer on the CPU frame owner, after acquisition/pacing and before simulation ends. */
RSF_RUNTIME_API int32_t rsf_gl_pipeline_input(rsf_gl_pipeline*, uint64_t frame, uint32_t kinds, uint32_t message);
RSF_RUNTIME_API uint32_t rsf_gl_pipeline_pcl_message(rsf_gl_pipeline*);
RSF_RUNTIME_API rsf_backend_result rsf_gl_pipeline_abort(rsf_gl_pipeline*, uint64_t frame);
RSF_RUNTIME_API rsf_backend_result rsf_gl_pipeline_evaluate(rsf_gl_pipeline*, const rsf_game_render_pass*);
RSF_RUNTIME_API rsf_backend_result rsf_gl_pipeline_hudless(rsf_gl_pipeline*, const rsf_game_render_pass*);
/* Every present carries its nonzero CPU frame ID, including menus. Missing scene
   captures disable FG; zero is never a substitute for the input frame identity. */
RSF_RUNTIME_API rsf_backend_result rsf_gl_pipeline_present(rsf_gl_pipeline*, uint64_t frame, uint32_t vsync);
RSF_RUNTIME_API rsf_backend_result rsf_gl_pipeline_status_get(rsf_gl_pipeline*, rsf_gl_pipeline_status*);
RSF_RUNTIME_API rsf_backend_result rsf_gl_pipeline_close(rsf_gl_pipeline*);
#ifdef __cplusplus
}
#endif
