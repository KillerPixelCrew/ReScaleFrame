// SPDX-License-Identifier: MIT
#pragma once
#include "motion_frame_target.h"
#include <rescaleframe/d3d11_fg_api.h>
#include <rescaleframe/d3d11_present_api.h>
#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
typedef struct IDXGISwapChain rsf_anomaly_fg_swap_chain;
#else
typedef void rsf_anomaly_fg_swap_chain;
#endif
typedef void (*rsf_anomaly_fg_overlay_present)(void* user, rsf_anomaly_fg_swap_chain* chain);

typedef struct rsf_d3d11_state rsf_d3d11_state;

typedef struct rsf_anomaly_fg_state_scope {
    uint32_t (*save)(void*, rsf_d3d11_state*);
    void (*restore)(void*, rsf_d3d11_state*);
} rsf_anomaly_fg_state_scope;

// The C leaves the FG stage needs: the guarded HW.pBaseZB read and the Runtime's D3D11 state scope.
typedef struct rsf_anomaly_fg_platform {
    // Selected from the fingerprinted profile. Returns a borrowed depth view.
    void* (*engine_depth_view)(uint32_t rva);
    rsf_anomaly_fg_state_scope state_scope;
} rsf_anomaly_fg_platform;

#ifdef __cplusplus
extern "C"
#endif
bool rsf_anomaly_fg_configure_ini(const wchar_t*, bool apply, void* runtime, uint32_t depth_rva,
    rsf_game_log_fn, const rsf_anomaly_fg_platform*)
#ifdef __cplusplus
    noexcept
#endif
    ;

#ifdef __cplusplus
#define RSF_FG_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_FG_NOEXCEPT
#endif
bool rsf_anomaly_fg_installed(void) RSF_FG_NOEXCEPT;
void rsf_anomaly_fg_set_overlay_sink(rsf_anomaly_fg_overlay_present, void*) RSF_FG_NOEXCEPT;
void rsf_anomaly_fg_stage_input_begin(uint32_t) RSF_FG_NOEXCEPT;
void rsf_anomaly_fg_stage_input_received(uint32_t kinds, uint32_t message) RSF_FG_NOEXCEPT;
void rsf_anomaly_fg_stage_report(rsf_game_log_fn emit, void* user) RSF_FG_NOEXCEPT;
#ifdef __cplusplus
}
#endif
#undef RSF_FG_NOEXCEPT
