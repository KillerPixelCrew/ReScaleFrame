/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/native_scene.h>
#include <rescaleframe/native_window.h>
#include <rescaleframe/overlay_d3d12.h>
#include <rescaleframe/native_cpu.h>
#include "d3d11_marker_bridge.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct rsf_ac7_loop {
    void *chain, *window, *gpu;
    rsf_game_render_pass* source;
    rsf_native_fg_options* options;
    uint32_t switching, pacing, ordering_only, visible;
    void* user;
    /* These calls lend the native command list through the live frame. */
    int (*begin)(void* user, const rsf_game_render_pass* source, uint32_t direct);
    int (*draw_present)(void* user, const rsf_game_render_pass* source, uint32_t direct, uint32_t late_simulation,
        int32_t* evaluated, int32_t* presented);
    int (*finish)(void* user);
    int (*status)(rsf_native_fg_status*);
    int (*read_cpu)(uint64_t session, uint64_t source, rsf_native_cpu_frame*);
} rsf_ac7_loop;

int rsf_d3d11_fixture_run_loop(const rsf_ac7_loop*);
void rsf_d3d11_fixture_cpu_event(uint64_t id, uint32_t stage);
int rsf_d3d11_fixture_token_matches(uint64_t id, uint32_t expected);

/* AC7 commands and exception handling stay in the native translation unit. */
typedef struct rsf_ac7_fixture_calls {
    void (*install_exception_filter)(void);
    const rsf_d3d11_marker_fixture_calls* marker_calls;
    int (*run_guarded)(rsf_ac7_loop*, void* device, void* context,
        void* hudless, void* hudless_target, rsf_dlss_frame*, uint32_t stall_ms);
} rsf_ac7_fixture_calls;

void rsf_ac7_fixture_log(void*, const char*);
void rsf_ac7_fixture_marker_configure(void);
uint64_t rsf_ac7_fixture_marker_stamp(void);
uint64_t rsf_ac7_fixture_marker_frequency(void);
void* rsf_ac7_fixture_marker_token(uint64_t id);
int rsf_ac7_fixture_marker_sleep_count(uint64_t* count);
void rsf_ac7_fixture_marker_reenter(uint64_t id);
uint32_t rsf_ac7_fixture_marker_reentries(void);

int rsf_d3d11_fixture_main(int argc, char** argv, const rsf_ac7_fixture_calls*);
int rsf_fg_fixture_main(int argc, char** argv);

#ifdef __cplusplus
}
#endif
