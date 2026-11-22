/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/ac7_render_scope.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct rsf_d3d11_scope_fixture_calls {
    int (*open)(rsf_ac7_render_scopes*, void*, const rsf_game_render_pass*, rsf_ac7_render_ticket**);
    int (*close)(rsf_ac7_render_ticket*, void*);
} rsf_d3d11_scope_fixture_calls;

/* Returned ticket is borrowed from scopes and belongs to the supplied command list. */
int rsf_d3d11_fixture_scope_begin(const rsf_d3d11_scope_fixture_calls*, rsf_ac7_render_scopes*,
    void* commands, const rsf_game_render_pass*, rsf_ac7_render_ticket** frame);
int rsf_d3d11_fixture_scope_scene(const rsf_d3d11_scope_fixture_calls*, rsf_ac7_render_scopes*,
    void* commands, const rsf_game_render_pass*, rsf_ac7_render_ticket** submission);
int rsf_d3d11_fixture_scope_window(const rsf_d3d11_scope_fixture_calls*, rsf_ac7_render_scopes*,
    void* commands, const rsf_game_render_pass*, rsf_ac7_render_ticket* submission,
    void* backbuffer, void* hudless, void* window, void* chain, uint32_t direct,
    rsf_ac7_render_ticket** output);

#ifdef __cplusplus
}
#endif
