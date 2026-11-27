// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_api.h>
#ifdef __cplusplus
extern "C" {
#endif
#define RSF_CALLBACK_HOST_ABI_VERSION 1u
#define RSF_CALLBACK_HOST_ABI_VERSION_V2 2u
/* GPU services for adapters whose callbacks guarantee command-slot retirement.
   Engine startup, symbols and configuration translation stay in the adapter. */
typedef struct rsf_callback_host_setup {
    uint32_t struct_size, abi_version;
    const wchar_t* plugin_path;
    const wchar_t* log_path;
    const wchar_t* preferences_path;
    const rsf_game_probe* probe;
    uint32_t backend, quality, dlss_preset;
    uint32_t sr_backend_choices;
    uint32_t engine_type;
    const char* engine_version_utf8;
    const char* project_id_utf8;
    const char* streamline_directory_utf8;
    const char* fsr2_directory_utf8;
    const char* fsr3_directory_utf8;
    const char* fsr4_directory_utf8;
    const char* xess_directory_utf8;
    int (*set_engine_spatial)(uint32_t spatial, uint32_t quality);
} rsf_callback_host_setup;
/* Supply &v2.base to start, with its size set to sizeof(v2) and version set to V2.
   The policy request returns an adapter generation. The adapter reports that generation
   only during the RECONFIGURE callback that proves retirement of earlier GPU work. */
typedef struct rsf_callback_host_setup_v2 {
    rsf_callback_host_setup base;
    int (*request_policy)(uint32_t spatial, uint32_t quality, uint32_t* generation);
    int (*drained_generation)(uint32_t* generation);
    int (*spatial_extent)(uint32_t quality, uint32_t output_width, uint32_t output_height,
        uint32_t* render_width, uint32_t* render_height);
    const char* spatial_name_utf8;
} rsf_callback_host_setup_v2;
/* An adapter that owns its plugin lifecycle (a Unity package the engine loads) attaches instead:
   supply &v3.base with its size set to sizeof(v3) and version V3, plugin_path and probe null,
   log_path and preferences_path set. The host fills *services for the adapter's renderer bridge
   and serves rsf_render_settings_get/apply; rsf_callback_host_stop retires it. A policy change
   is applied on the pass the adapter flags RECONFIGURE after retiring its earlier work. */
#define RSF_CALLBACK_HOST_ABI_VERSION_V3 3u
typedef struct rsf_callback_host_setup_v3 {
    rsf_callback_host_setup base;
    rsf_game_host_services* services;
} rsf_callback_host_setup_v3;
/* The host's SR facts for an adapter's own status API. The backends are rsf_sr_backend ids:
   requested is the selected one (Auto stays Auto), active the one the current plan serves
   (none while no plan is enabled). */
typedef struct rsf_callback_host_status {
    uint32_t struct_size;
    uint32_t requested_backend, active_backend, enabled;
    int32_t result;
    uint32_t render_width, render_height, output_width, output_height;
    uint64_t evaluated_frames, refused_frames;
} rsf_callback_host_status;
RSF_RUNTIME_API uint32_t rsf_callback_host_start(const rsf_callback_host_setup*);
RSF_RUNTIME_API uint32_t rsf_callback_host_stop(void);
/* 1 with the published host's facts, 0 without a host or a short structure. */
RSF_RUNTIME_API uint32_t rsf_callback_host_status_get(rsf_callback_host_status*);
#ifdef __cplusplus
}
#endif
