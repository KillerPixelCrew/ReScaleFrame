// SPDX-License-Identifier: MIT
#pragma once
#include "fg_inputs_platform.h"
#include "fg_overlay.h"
#include <rescaleframe/d3d11_fg_api.h>
#include <rescaleframe/d3d11_present_api.h>
#include <stdbool.h>
#include <stddef.h>

typedef struct rsf_anomaly_fg_config {
    bool bridge, enabled, debug, development, capture;
    uint32_t backend, generated, latency, frame_limit_us, scope_hold;
    uint32_t capture_start, capture_count, capture_interval;
    const wchar_t *capture_dir, *streamline_dir, *fidelityfx_dir, *xess_dir;
} rsf_anomaly_fg_config;

typedef struct rsf_anomaly_fg_status {
    bool installed, owner;
    uint32_t backend, choices, requested_mode, effective_mode, generated, reason;
    uint64_t source_frames, level_frames, tagged, fed, presented, unpresented, runtime_tagged,
        vendor_presented, captures;
} rsf_anomaly_fg_status;

typedef struct rsf_anomaly_fg_motion_target {
    void* texture;
    uint32_t width, height, frame;
    bool written, main;
} rsf_anomaly_fg_motion_target;

typedef void (*rsf_anomaly_fg_overlay_present)(void* user, rsf_anomaly_fg_swap_chain* chain);
typedef void* (*rsf_anomaly_fg_depth_source)(void);

typedef struct rsf_anomaly_fg_platform {
    // Immutable input pass and engine metadata shared with the SR owner.
    const rsf_anomaly_input_platform* inputs;
    const char* engine_version;
    const char* project_id;
    float units_to_meters;
    void (*publish_overlay)(const rsf_anomaly_fg_overlay_link* link);
    // Selected from the fingerprinted profile. Returns a borrowed depth view.
    void* (*engine_depth_view)(uint32_t rva);
    bool (*sr_owns_depth)(void);
    rsf_anomaly_fg_motion_target (*motion_target)(void);
    // Owned COM references; released with inputs->release_interface.
    void* (*immediate_context)(void* chain);
    void* (*backbuffer)(void* chain);
    void* (*depth_texture)(void* view);
    bool (*output_window)(void* chain, uintptr_t* window);
} rsf_anomaly_fg_platform;

#ifdef __cplusplus
extern "C" {
bool rsf_anomaly_fg_configure_ini(const wchar_t*, bool apply, void* runtime, uint32_t depth_rva,
    rsf_game_log_fn, const rsf_anomaly_fg_platform*) noexcept;
void rsf_anomaly_fg_configure(const rsf_anomaly_fg_config*, rsf_game_log_fn, void*) noexcept;
bool rsf_anomaly_fg_install(void* runtime, uint32_t depth_rva, const rsf_anomaly_fg_platform*) noexcept;
bool rsf_anomaly_fg_install_with(const rsf_d3d11_present_api*, const rsf_d3d11_fg_api*,
    const rsf_anomaly_fg_platform*) noexcept;
bool rsf_anomaly_fg_installed() noexcept;
uint32_t rsf_anomaly_fg_get_scope_hold(uint32_t fallback) noexcept;
void rsf_anomaly_fg_set_scope_hold(uint32_t frames) noexcept;
void rsf_anomaly_fg_set_depth_source(rsf_anomaly_fg_depth_source) noexcept;
void rsf_anomaly_fg_set_overlay_sink(rsf_anomaly_fg_overlay_present, void*) noexcept;
void rsf_anomaly_fg_stage_camera(const float*, const float*) noexcept;
void rsf_anomaly_fg_stage_input_begin(uint32_t) noexcept;
void rsf_anomaly_fg_stage_input_received(uint32_t kinds, uint32_t message) noexcept;
void rsf_anomaly_fg_stage_render_begin(uint32_t engine_frame, uint32_t width, uint32_t height) noexcept;
void rsf_anomaly_fg_stage_scope() noexcept;
void rsf_anomaly_fg_stage_ui_boundary() noexcept;
void rsf_anomaly_fg_stage_end_before() noexcept;
void rsf_anomaly_fg_stage_end_after() noexcept;
void rsf_anomaly_fg_stage_device_reset() noexcept;
void rsf_anomaly_fg_stage_report(rsf_game_log_fn emit, void* user) noexcept;
rsf_anomaly_fg_status rsf_anomaly_fg_stage_status() noexcept;
}
#endif
