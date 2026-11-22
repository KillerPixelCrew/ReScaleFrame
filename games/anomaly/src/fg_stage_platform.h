// SPDX-License-Identifier: MIT
#pragma once
#include "motion_frame_target.h"
#include <rescaleframe/d3d11_fg_api.h>
#include <rescaleframe/d3d11_present_api.h>
#include <stdbool.h>
#include <stddef.h>

typedef struct rsf_anomaly_fg_status {
    bool installed, owner;
    uint32_t backend, choices, requested_mode, effective_mode, generated, reason;
    uint64_t source_frames, level_frames, eligible_at_ui_boundary, fed, presented, unpresented, runtime_tagged,
        vendor_presented, captures;
} rsf_anomaly_fg_status;

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
void rsf_anomaly_fg_stage_camera(const float*, const float*) RSF_FG_NOEXCEPT;
void rsf_anomaly_fg_stage_input_begin(uint32_t) RSF_FG_NOEXCEPT;
void rsf_anomaly_fg_stage_input_received(uint32_t kinds, uint32_t message) RSF_FG_NOEXCEPT;
void rsf_anomaly_fg_stage_render_begin(uint32_t engine_frame, uint32_t width, uint32_t height) RSF_FG_NOEXCEPT;
void rsf_anomaly_fg_stage_scope(void) RSF_FG_NOEXCEPT;
void rsf_anomaly_fg_stage_ui_boundary(void) RSF_FG_NOEXCEPT;
void rsf_anomaly_fg_stage_end_before(void) RSF_FG_NOEXCEPT;
void rsf_anomaly_fg_stage_end_after(void) RSF_FG_NOEXCEPT;
void rsf_anomaly_fg_stage_device_reset(void) RSF_FG_NOEXCEPT;
void rsf_anomaly_fg_stage_report(rsf_game_log_fn emit, void* user) RSF_FG_NOEXCEPT;
#ifdef __cplusplus
}
#endif
#undef RSF_FG_NOEXCEPT
