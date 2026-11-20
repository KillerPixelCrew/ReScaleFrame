/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>
#include <wchar.h>
#include <rescaleframe/d3d11_host.h>
#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/native_window.h>
#include <rescaleframe/overlay_api.h>
#include <rescaleframe/render_frontend.h>
#include "auxiliary_sr_platform.h"

#ifdef __cplusplus
extern "C" {
#endif

/* Private D3D11 host bridge. d3d11_host.rs owns admission, leases, lifecycle,
   recovery and status; every entry below is one existing native leaf call.
   Entries never throw. Returned COM objects carry one reference. The Rust
   mirror in d3d11_host_abi.rs is hand-written and checked by the size
   assertions in d3d11_host.cpp. */
typedef struct rsf_d3d11h_strings {
    const char *streamline, *fidelityfx, *xess, *captures;
    const char *identity, *fingerprint, *engine_version, *project;
} rsf_d3d11h_strings;

typedef struct rsf_d3d11h_bridge {
    uint32_t mode, rendered, allowed_backend_choices, max_generated_frames;
    const rsf_d3d11_host_setup* setup; /* numeric metadata; strings come from below */
    const rsf_d3d11h_strings* strings;
    rsf_game_log_fn log;
    void* user;
    int (*accept_window)(void* user, void* window);
    void (*present_event)(void* user, const rsf_observer_present_event*);
} rsf_d3d11h_bridge;

typedef struct rsf_d3d11h_pipeline_start {
    uint32_t output_width, output_height, quality, reserved;
    const rsf_d3d11_host_setup* setup;
    const rsf_d3d11h_strings* strings;
    rsf_game_log_fn log;
    void* user;
} rsf_d3d11h_pipeline_start;

typedef struct rsf_d3d11h_pipeline_status {
    uint32_t running, output_width, output_height, render_width, render_height;
} rsf_d3d11h_pipeline_status;

typedef void (*rsf_d3d11h_present_fn)(void* user, const rsf_observer_present_event*);

typedef struct rsf_d3d11h_platform {
    /* COM, D3D11 and DXGI queries. */
    int32_t (*same_object)(void* left, void* right);
    void* (*child_device)(void* child);
    uint32_t (*is_immediate)(void* context);
    uint32_t (*device_ok)(void* device);
    void* (*immediate_context)(void* device);
    int32_t (*device_vendor)(void* device, uint32_t* vendor);
    void* (*chain_device)(void* chain);
    void* (*chain_window)(void* chain);
    void* (*chain_buffer)(void* chain);
    /* Presentation bridge, Present observation and owned-chain continuation. */
    int32_t (*install_bridge)(const rsf_d3d11h_bridge*);
    int32_t (*observer_install)(rsf_game_log_fn, void* user, rsf_d3d11h_present_fn);
    int32_t (*present_is_owner)(void* chain);
    void (*chain_dispatch)(rsf_d3d11_chain_dispatch*);
    int32_t (*creation_begin)(void* window, uint64_t* token); /* HRESULT */
    int32_t (*creation_end)(void* window, uint64_t token, void* device, void* context, void* outer); /* HRESULT */
    int32_t (*wrap_chain)(const rsf_d3d11_native_chain*, rsf_game_log_fn, void* user); /* rsf_result */
    /* Native scene, window, FG and CPU owners. */
    void (*fg_frame)(const rsf_game_render_pass*, uint32_t begin);
    void (*fg_submission)(const rsf_game_render_pass*, uint32_t begin);
    void (*fg_final)(void* context, const rsf_game_render_pass*);
    int32_t (*fg_capture)(void* context, const rsf_game_render_pass*, const rsf_motion_decode_params*);
    int32_t (*scene_pass)(const rsf_game_render_pass*, uint32_t begin);
    uint32_t (*scene_present_path)(const rsf_game_render_pass*);
    int32_t (*window_scope)(const rsf_game_render_pass*, uint32_t begin);
    int32_t (*window_present)(const rsf_observer_present_event*);
    int32_t (*window_last_present)(rsf_native_present_record*);
    int32_t (*window_match)(void* chain, rsf_game_render_pass*);
    void (*latency_window_bind)(uint64_t session_id, void* window);
    int32_t (*cpu_dispatch_fg)(const rsf_game_cpu_event*);
    void (*cpu_event)(const rsf_game_cpu_event*);
    /* Streamline-hosted SR pipeline and native Present bookkeeping. */
    int32_t (*dlss_native_present)(void* chain, uint32_t sync_interval, uint32_t flags, uint32_t begin);
    int32_t (*pipeline_start)(void* device, const rsf_d3d11h_pipeline_start*);
    int32_t (*pipeline_resize)(uint32_t width, uint32_t height);
    int32_t (*pipeline_select)(uint32_t backend);
    int32_t (*pipeline_quality)(uint32_t quality);
    int32_t (*pipeline_preset)(uint32_t preset);
    void (*pipeline_sharpen)(const rsf_render_settings*);
    int32_t (*pipeline_status)(rsf_d3d11h_pipeline_status*);
    void (*pipeline_colour)(uint32_t transport, uint32_t correction);
    void* (*pipeline_output)(void);
    int32_t (*sr_evaluate)(void* context, const rsf_game_render_pass*);
    int32_t (*sr_destinations_ordered)(void);
    /* Frame capture. */
    void (*dump_configure)(const rsf_d3d11h_strings*, rsf_game_log_fn, void* user);
    void (*dump_request)(void);
    int32_t (*dump_requested)(void);
    void (*dump_poll)(void);
    int32_t (*dump_begin)(const rsf_frame_record*, uint32_t backend);
    int32_t (*dump_wants)(const rsf_frame_record*);
    void (*dump_inputs)(void* context, const rsf_game_render_pass*, const rsf_frame_record*, void* original_colour);
    void (*dump_pass)(void* context, const rsf_game_render_pass*, uint32_t begin);
    int32_t (*dump_texture)(void* context, const rsf_frame_record*, const char* name, const char* description,
        void* texture, uint32_t view, float scale);
    void (*dump_result)(const rsf_frame_record*, const char* stage, uint32_t backend, int32_t result);
    void (*dump_complete)(const rsf_frame_record*, int32_t result);
    /* Shared overlay. configure resolves the overlay table and reads the applied settings. */
    int32_t (*overlay_configure)(const rsf_overlay_service_setup*, rsf_render_settings* applied);
    void (*overlay_present)(void* chain);
    void (*overlay_poll_toggle)(void);
    /* Adapter INI keys. */
    int32_t (*ini_read)(const wchar_t* path, const wchar_t* section, const wchar_t* key, int32_t fallback);
    int32_t (*ini_write)(const wchar_t* path, const wchar_t* section, const wchar_t* key, const wchar_t* value);
    /* Capture budgets kept under their original license in failure_capture_policy.h. */
    int32_t (*failure_claim)(uint32_t enabled, uint32_t request_pending, int32_t result);
    void (*sequence_request)(void);
    int32_t (*sequence_claim)(uint64_t session_id, uint64_t source_frame_id, uint32_t ready, uint32_t request_pending);
    /* Existing auxiliary SR owner. */
    const rsf_aux_platform* auxiliary;
} rsf_d3d11h_platform;

#ifdef __cplusplus
}
#endif
