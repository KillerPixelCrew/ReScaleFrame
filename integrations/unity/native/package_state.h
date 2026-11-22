/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal package ABI. Rust owns package state, routing, SR planning and the session
   and Unity device lifecycles. The C11 leaf package.c supplies the Unity SDK calls; the rsfu_*
   exports are Rust (native/bridge.def). Settings, status and frame-counter layouts are also used by Runtime/Native.cs. */
#include <rescaleframe/game_renderer.h>
#include <rescaleframe/sharpening_settings.h>
#include <rescaleframe/unity_bridge.h>
#include <stdint.h>
#include <wchar.h>

#define RSF_UNITY_SETTINGS_ABI_VERSION 2u
#define RSF_UNITY_PACKAGE_ABI_VERSION 2u
#define RSF_UNITY_DEVICE_EVENT_INITIALIZE 0u
#define RSF_UNITY_DEVICE_EVENT_SHUTDOWN 1u
typedef struct rsf_unity_package_settings {
    uint32_t struct_size, abi_version;
    uint32_t sr_backend, quality, dlss_preset;
    uint32_t fg_backend, fg_mode, generated_frames, latency_mode, frame_limit_us;
    uint32_t gameplay;
    rsf_sharpening_options sharpening;
} rsf_unity_package_settings;
typedef struct rsf_unity_package_status {
    uint32_t struct_size, abi_version;
    uint32_t requested_sr, active_sr, render_width, render_height, output_width, output_height;
    int32_t sr_result, fg_result;
    uint32_t fg_backend, fg_available, fg_active, max_generated_frames, generated_frames;
    uint32_t latency_available, latency_mode, reason;
    uint64_t evaluated_frames, refused_frames, tagged_frames;
    uint64_t sleep_calls, marker_calls;
    uint32_t pcl_available, latency_report_available;
    uint32_t latency_owner;
    uint64_t keyboard_inputs, mouse_inputs, controller_inputs, pcl_pings, input_substitutes, order_violations, derived_boundaries;
} rsf_unity_package_status;
typedef struct rsf_unity_frame_counters {
    uint32_t struct_size, abi_version, valid, active;
    uint64_t total_presented, generated_presented, provider_generation;
} rsf_unity_frame_counters;

/* The Runtime SR context a view asks for: its requested backend (0 is the spatial or native path),
   the stored quality and DLSS model, the pass camera's extents and depth convention, and the
   package identity as UTF-8, borrowed for the call: the SDK root, its streamline, fidelityfx and
   xess directories, the Streamline interposer, Unity's version and the project id. The Rust
   leaf adds the device, Streamline host and log. */
typedef struct rsf_unity_package_view_setup {
    uint32_t backend, quality, dlss_preset;
    uint32_t output_width, output_height, render_width, render_height;
    uint32_t inverted_depth;
    const char* root_utf8;
    const char* streamline_utf8;
    const char* interposer_utf8;
    const char* fsr_utf8;
    const char* xess_utf8;
    const char* engine_version_utf8;
    const char* project_id_utf8;
} rsf_unity_package_view_setup;
#ifdef __cplusplus
extern "C" {
#endif
void rsf_unity_package_plugin_load(void* interfaces);
void rsf_unity_package_plugin_unload(void);
void rsf_unity_package_device_event(uint32_t sdk_event);
#ifdef __cplusplus
}
#endif