/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/ac7_render_scope.h>
#include <rescaleframe/dlss.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct rsf_d3d11_scope_fixture_calls {
    int (*open)(rsf_ac7_render_scopes*, void*, const rsf_game_render_pass*, rsf_ac7_render_ticket**);
    int (*close)(rsf_ac7_render_ticket*, void*);
} rsf_d3d11_scope_fixture_calls;

/* Returned ticket is borrowed from scopes and belongs to the supplied command list. */
int rsf_d3d11_fixture_scope_begin(const rsf_d3d11_scope_fixture_calls*, rsf_ac7_render_scopes*,
    void* commands, const rsf_game_render_pass*, rsf_ac7_render_ticket** frame,
    uint32_t direct, uint32_t* expected_path);

typedef struct rsf_ac7_fixture_commands {
    const rsf_d3d11_scope_fixture_calls* scopes_api;
    rsf_ac7_render_scopes* scopes;
    rsf_ac7_render_ticket* frame_ticket;
    void *commands, *chain, *hudless, *window;
    rsf_dlss_frame* frame;
    void *evaluate, *present, *simulation_end;
    /* The native draw owns the backbuffer and RTV until the frame executes. */
    void* draw_user;
    int (*draw)(void* user, void** backbuffer);
    /* Native execution invokes each command under the fixture's exception guard. */
    void (*execute)(void* commands);
} rsf_ac7_fixture_commands;

int rsf_d3d11_fixture_record(const rsf_ac7_fixture_commands*, const rsf_game_render_pass*,
    uint32_t direct, uint32_t late_simulation);
int rsf_d3d11_fixture_queued_pass(void* context, uint32_t expected_path,
    const rsf_game_render_pass*, uint32_t begin);
int rsf_d3d11_fixture_window_source(void* chain, uint32_t expected_path, uint64_t* source);

#ifdef __cplusplus
}
#endif
