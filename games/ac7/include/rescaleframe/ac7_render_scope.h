/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_AC7_RENDER_SCOPE_H
#define RSF_AC7_RENDER_SCOPE_H
#include <stdint.h>
#include <rescaleframe/game_renderer.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct rsf_ac7_render_scopes rsf_ac7_render_scopes;
typedef struct rsf_ac7_render_ticket rsf_ac7_render_ticket;

/* Engine identity copied while the graph owns it. No engine pointer is borrowed by a consumer.
   Native family frames are separate from input/simulation frame IDs and cannot authorize FG. */
typedef rsf_game_render_pass rsf_ac7_render_scope;

/* Roles 0 and 6 are unassigned; zero is also the value of a pass with no role. */
#define RSF_AC7_ROLE_SR 1u
#define RSF_AC7_ROLE_TONEMAP 2u
#define RSF_AC7_ROLE_AA 3u
#define RSF_AC7_ROLE_MATERIAL 4u
#define RSF_AC7_ROLE_HUD 5u
#define RSF_AC7_ROLE_COMPOSITE 7u
#define RSF_AC7_ROLE_OUTPUT 8u

/* Runs on the engine's RHI execution stream. The callback must restore graphics state and must
   not retain scope. begin=0 restores the enclosing scope, or closes the outermost one. */
typedef void (*rsf_ac7_render_scope_fn)(void* user, void* command_list,
                                       const rsf_ac7_render_scope* scope, uint32_t begin);
typedef struct rsf_ac7_scope_lease {
    void* object;
    void (*resolve)(void* object, rsf_ac7_render_scope* scope);
    void (*release)(void* object);
} rsf_ac7_scope_lease;

int rsf_ac7_render_scopes_create(uint64_t session_id, uint32_t capacity,
                                 rsf_ac7_render_scope_fn callback, void* user,
                                 rsf_ac7_render_scopes** out);

/* Execution callbacks identify the same ticket at begin/end. The independent state callback
   preserves current-scope restoration notifications. All resources resolve again before end. */
int rsf_ac7_render_scopes_create_passes(uint64_t session_id, uint32_t capacity,
    rsf_ac7_render_scope_fn state_callback, rsf_ac7_render_scope_fn pass_callback,
    void* user, rsf_ac7_render_scopes** out);

/* Private AC7 command-list layout, verified against the matching dump. The caller must own the
   list on the render thread. Reserve both markers before beginning, so close cannot fail OOM.
   Native commands queued between open and close execute inside the copied pass/view scope. */
int rsf_ac7_render_scope_open(rsf_ac7_render_scopes*, void* native_command_list,
                              const rsf_ac7_render_scope*, rsf_ac7_render_ticket** out);
/* Takes lease ownership on success only. Resolution occurs on the RHI stream, release after end. */
int rsf_ac7_render_scope_open_leased(rsf_ac7_render_scopes*, void* native_command_list,
    const rsf_ac7_render_scope*, const rsf_ac7_scope_lease*, rsf_ac7_render_ticket** out);
int rsf_ac7_render_scope_close(rsf_ac7_render_ticket*, void* native_command_list);

/* A snapshot for the current graphics stream. Zero means no identified engine scope. */
int rsf_ac7_render_scope_current(rsf_ac7_render_scopes*, rsf_ac7_render_scope* out);
/* Disable further producers. Existing queued markers continue to close safely. */
void rsf_ac7_render_scopes_quiesce(rsf_ac7_render_scopes*);
/* Readiness check without releasing the owner. Used before disabling native drain hooks. */
int rsf_ac7_render_scopes_idle(rsf_ac7_render_scopes*);
/* Refuses destruction while markers/scopes remain. The module must remain loaded until success. */
int rsf_ac7_render_scopes_destroy(rsf_ac7_render_scopes*);

#ifdef __cplusplus
}
#endif
#endif
