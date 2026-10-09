/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_AC7_NATIVE_RENDERER_H
#define RSF_AC7_NATIVE_RENDERER_H
#include <rescaleframe/ac7_render_scope.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_ac7_native_renderer rsf_ac7_native_renderer;
typedef void (*rsf_ac7_renderer_log_fn)(void*, const char*);
/* Evidence-only hooks on the pixel uniform binds. They run for every bind of every draw, so they
   are installed only when asked for here or by RSF_AC7_PIXEL_DIAGNOSTICS=1 in the environment. */
#define RSF_AC7_DIAG_PIXEL_BINDINGS 0x1u
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
    uint32_t diagnostics; /* RSF_AC7_DIAG_*; zero for none */
} rsf_ac7_native_renderer_options;
/* After decryption, before graphics activation. Expected bytes are checked before any hook.
   Owns native pass boundaries and queued metadata; no vendor library is loaded here. */
int rsf_ac7_native_renderer_prepare(const rsf_ac7_native_renderer_options*, rsf_ac7_native_renderer** out);
int rsf_ac7_native_renderer_start(rsf_ac7_native_renderer*);
int rsf_ac7_native_renderer_is_active(rsf_ac7_native_renderer*);
void rsf_ac7_native_renderer_quiesce(rsf_ac7_native_renderer*);
int rsf_ac7_native_renderer_stop(rsf_ac7_native_renderer*);
#ifdef __cplusplus
}
#endif
#endif
