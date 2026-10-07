/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_AC7_NATIVE_RENDERER_H
#define RSF_AC7_NATIVE_RENDERER_H
#include <rescaleframe/ac7_render_scope.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_ac7_native_renderer rsf_ac7_native_renderer;
/* Called synchronously; message storage is borrowed for the callback duration. */
typedef void (*rsf_ac7_renderer_log_fn)(void*, const char*);
/* Copyable preparation options. Set struct_size and a nonzero session_id. A zero capacity uses
   256 queued scopes; explicit capacity is 1..4096. Callbacks/user must survive until stop succeeds.
   render_config is queried on engine producers; on_stage/on_state run on RHI execution. */
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
/* Prepare after executable decryption and before graphics activation. Returns 1 after installing
   inactive forwarding hooks, 0 after refusal. Expected bytes guard every entry/helper. A failed
   partial activation may return a retained controller in *out; the caller must keep it owned.
   Owns native boundaries and queued metadata; vendor SDK loading belongs to the runtime. */
int rsf_ac7_native_renderer_prepare(const rsf_ac7_native_renderer_options*, rsf_ac7_native_renderer** out);
/* Activate an installed, non-quiescing controller after its scene-format patch succeeds. */
int rsf_ac7_native_renderer_start(rsf_ac7_native_renderer*);
/* Live controller activity, including deactivation after a command/resource ownership refusal. */
int rsf_ac7_native_renderer_is_active(rsf_ac7_native_renderer*);
/* Close producer admission. Native drain observers remain installed until stop can release them. */
void rsf_ac7_native_renderer_quiesce(rsf_ac7_native_renderer*);
/* Returns 1 after freeing the controller (also for null), 0 while work, callbacks or guarded
   restoration prevent release. Quiesce first; retain the controller, callbacks and module on 0. */
int rsf_ac7_native_renderer_stop(rsf_ac7_native_renderer*);
/* Copy the current RHI scope into size-initialized output; returns 0 outside an identified scope. */
int rsf_ac7_native_renderer_current(rsf_ac7_native_renderer*, rsf_ac7_render_scope*);
#ifdef __cplusplus
}
#endif
#endif
