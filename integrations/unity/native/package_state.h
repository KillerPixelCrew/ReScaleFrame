/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal package ABI. Rust owns package state, routing, SR planning and the session
   and Unity device lifecycles. The C11 leaf package.c supplies the rsfu_* exports and the Unity SDK
   calls. Settings, status and frame-counter layouts are also used by Runtime/Native.cs. */
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
/* The bodies of rsfu_configure, rsfu_settings, rsfu_status, rsfu_frame_counters and
   rsfu_temporal_ready, with the same checks and result codes. */
int rsf_unity_package_configure(const rsf_unity_package_settings* input);
int rsf_unity_package_settings_get(rsf_unity_package_settings* output);
int rsf_unity_package_status_get(rsf_unity_package_status* output);
int rsf_unity_package_frame_counters(rsf_unity_frame_counters* output);
uint32_t rsf_unity_package_temporal_ready(void);

/* Session lifecycle. initialize retires a started or partially opened session once the bridge has drained and the
   Runtime has ended its session (2 while either is pending), forgets the session's plan, status
   and bridge failure, stores rsfu_initialize's identity (the UTF-8 SDK root, Unity version and
   project id; 0 when one or output is null) and opens the new session: the per-process log
   unless a log is open, the Editor's generation install, the player's input window while none
   is hooked, the player's Vulkan surface, and the bridge prepared with this session's host
   services (0 when the surface or bridge refuses). It then activates the bridge with the
   persisted reconstruction request and writes the bridge's managed API to output (1). Settings
   persist across sessions. */
int rsf_unity_package_initialize(const char* runtime, const char* engine, const char* project,
    rsf_unity_native_api* output);

void rsf_unity_package_plugin_load(void* interfaces);
void rsf_unity_package_plugin_unload(void);
void rsf_unity_package_device_event(uint32_t sdk_event);

int rsf_unity_package_input(uint64_t frame, uint32_t kinds);
int rsf_unity_package_preview_region(int32_t left, int32_t top, int32_t width, int32_t height, uint32_t visible);
uint32_t rsf_unity_package_shutdown(void);
#ifdef __cplusplus
}
#endif