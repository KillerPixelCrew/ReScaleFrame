/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/native_cpu.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct rsf_d3d11_marker_fixture_calls {
    void (*configure)(void);
    uint64_t (*stamp)(void);
    uint64_t (*frequency)(void);
    int (*dispatch)(const rsf_game_cpu_event*);
    int (*read)(uint64_t session, uint64_t source, rsf_native_cpu_frame*);
    int (*counters)(rsf_native_cpu_counters*);
    void (*frame)(const rsf_game_render_pass*, uint32_t);
    void (*submit)(const rsf_game_render_pass*, uint32_t);
    void (*ordering)(uint32_t);
    void (*render)(uint64_t, uint32_t);
    void (*present)(uint64_t, void*, uint32_t);
    int32_t (*abort_frame)(uint64_t);
    int (*latency_frame)(rsf_native_latency_frame*);
    uint64_t (*generation)(void);
    void* (*token)(uint64_t);
    int (*sleep_count)(uint64_t*);
    void (*reenter)(uint64_t);
    uint32_t (*reentries)(void);
} rsf_d3d11_marker_fixture_calls;

int rsf_d3d11_marker_fixture(void* chain, const rsf_d3d11_marker_fixture_calls*);

#ifdef __cplusplus
}
#endif
