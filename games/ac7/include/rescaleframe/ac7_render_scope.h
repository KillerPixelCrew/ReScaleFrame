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

#define RSF_AC7_ROLE_SCENE 0u
#define RSF_AC7_ROLE_SR 1u
#define RSF_AC7_ROLE_TONEMAP 2u
#define RSF_AC7_ROLE_AA 3u
#define RSF_AC7_ROLE_MATERIAL 4u
#define RSF_AC7_ROLE_HUD 5u
#define RSF_AC7_ROLE_UI_FILTER 6u
#define RSF_AC7_ROLE_COMPOSITE 7u
#define RSF_AC7_ROLE_OUTPUT 8u

/* Runs on the engine's RHI execution stream; restore graphics state and do not retain scope.
   State callbacks at begin=0 receive the enclosing scope (zeroed at outermost close); pass
   callbacks receive their own original ticket identity at both begin=1 and end=0. */
typedef void (*rsf_ac7_render_scope_fn)(void* user, void* command_list,
                                       const rsf_ac7_render_scope* scope, uint32_t begin);
/* Resource owner transferred by a successful open. resolve refreshes borrowed resource fields
   immediately before each execution callback; release runs after the end marker on the RHI stream. */
typedef struct rsf_ac7_scope_lease {
    void* object;
    void (*resolve)(void* object, rsf_ac7_render_scope* scope);
    void (*release)(void* object);
} rsf_ac7_scope_lease;

/* Create an accepting owner with nonzero session and capacity 1..4096. Returns 1 on success,
   0 on invalid arguments/allocation failure. Callback/user remain borrowed until destruction. */
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
/* Append the reserved end marker to a recording list exactly once. Returns 0 if append fails;
   keep the ticket and owner alive and retry while the list can still accept native commands. */
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
