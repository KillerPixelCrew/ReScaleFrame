/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_AC7_NATIVE_RENDERER_H
#define RSF_AC7_NATIVE_RENDERER_H
#include <rescaleframe/ac7_render_scope.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_ac7_native_renderer rsf_ac7_native_renderer;
typedef void (*rsf_ac7_renderer_log_fn)(void*, const char*);
typedef struct rsf_ac7_native_renderer_options {
    uint32_t struct_size;
    uint32_t pending_capacity;
    uint64_t session_id;
    rsf_ac7_render_scope_fn on_stage;
    rsf_ac7_renderer_log_fn log;
    void* user;
    rsf_game_render_config_fn render_config;
    rsf_game_cpu_event_fn cpu_event;
    rsf_ac7_render_scope_fn on_state;
} rsf_ac7_native_renderer_options;
/* After decryption, before graphics activation. Expected bytes are checked before any hook.
   Owns native pass boundaries and queued metadata; no vendor library is loaded here. */
int rsf_ac7_native_renderer_prepare(const rsf_ac7_native_renderer_options*, rsf_ac7_native_renderer** out);
int rsf_ac7_native_renderer_start(rsf_ac7_native_renderer*);
int rsf_ac7_native_renderer_is_active(rsf_ac7_native_renderer*);
void rsf_ac7_native_renderer_quiesce(rsf_ac7_native_renderer*);
int rsf_ac7_native_renderer_stop(rsf_ac7_native_renderer*);
int rsf_ac7_native_renderer_current(rsf_ac7_native_renderer*, rsf_ac7_render_scope*);
#ifdef __cplusplus
}
#endif
#endif
