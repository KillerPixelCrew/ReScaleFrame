/* SPDX-License-Identifier: MIT */
#ifndef RSF_AC7_NATIVE_RENDERER_H
#define RSF_AC7_NATIVE_RENDERER_H
#include <rescaleframe/ac7_render_scope.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_ac7_native_renderer rsf_ac7_native_renderer;
typedef void (*rsf_ac7_renderer_log_fn)(void*, const char*);
/* The host services the adapter hands its renderer (lifecycle.rs). Evidence-only hooks on the
   pixel uniform binds run for every bind of every draw, so they are installed only when
   RSF_AC7_PIXEL_DIAGNOSTICS=1 is set in the environment or the [Startup] section of AC7.ini. */
typedef struct rsf_ac7_native_renderer_options {
    uint64_t session_id;
    rsf_ac7_render_scope_fn on_stage;
    rsf_ac7_renderer_log_fn log;
    void* user;
    rsf_game_render_config_fn render_config;
    rsf_game_cpu_event_fn cpu_event;
    rsf_ac7_render_scope_fn on_state;
} rsf_ac7_native_renderer_options;
#ifdef __cplusplus
}
#endif
#endif
