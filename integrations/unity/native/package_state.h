/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal boundary between the Unity package's native leaves (package.cpp) and their Rust state
   owner (games/unity-mono/src/package_state.rs), which lives in the rsf-unity-mono archive the
   package already links. Not a DLL ABI. tools/generate-rust-bindings.py generates
   games/unity-mono/src/package_abi.rs from this header.

   The settings, status and frame-counter structures are the package's managed contract
   (Runtime/Native.cs, rsfu_*) and keep their layout. Rust owns the stored settings and their
   validation, the status snapshot, the render plan, temporal readiness, the pending
   reconfiguration, the session lifecycle with its result codes, and the D3D12 SR views of the
   render callback (package_views.rs): one Runtime SR context per Unity view key, its backend
   choice and fallback, its replacement on new extents or reconfiguration, and the recording
   slots bound to the bridge's command lists. package.cpp keeps the Unity, DXGI, Vulkan and
   preview work, the render callback's routing and the input window, and lends the typed SDK and
   Runtime calls through rsf_unity_package_platform. package.cpp's mutex still serializes the
   exports and the render path; nothing here takes it. Rust never holds its state lock while it
   calls a leaf; its view lock is held across the SR leaves, which never re-enter, and is always
   taken before the state lock. */
#include <rescaleframe/game_renderer.h>
#include <rescaleframe/sharpening_settings.h>
#include <stdint.h>

#define RSF_UNITY_SETTINGS_ABI_VERSION 2u
#define RSF_UNITY_PACKAGE_ABI_VERSION 2u
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

/* Defined by rescaleframe/native_fg.h and native_cpu.h, which package.cpp includes. */
typedef struct rsf_native_fg_options rsf_native_fg_options;
typedef struct rsf_native_fg_status rsf_native_fg_status;
typedef struct rsf_native_latency_status rsf_native_latency_status;
/* Defined by rescaleframe/native_sr_d3d12.h; a Runtime SR context. */
typedef struct rsf_sr12 rsf_sr12;

/* The Runtime SR context a view asks for: its resolved backend (0 is the spatial or native path),
   the stored quality and DLSS model, the pass camera's extents and depth convention. package.cpp
   adds the device, SDK directories, Streamline host and log. */
typedef struct rsf_unity_package_view_setup {
    uint32_t backend, quality, dlss_preset;
    uint32_t output_width, output_height, render_width, render_height;
    uint32_t inverted_depth;
} rsf_unity_package_view_setup;

/* package.cpp's typed leaves, all present. No leaf calls back into a rsf_unity_package_* entry
   except open, which may report rsf_unity_package_fg_result; none may throw. */
typedef struct rsf_unity_package_platform {
    /* rsf::sharpening_settings::valid and rsf_dlss_preset_valid are header-inline C/C++. */
    uint32_t (*sharpening_valid)(const rsf_sharpening_options* options);
    uint32_t (*dlss_preset_valid)(uint32_t preset);
    /* The renderer bridge's engine policy, rsf_unity_set_engine_spatial. */
    int (*set_engine_spatial)(uint32_t spatial, uint32_t quality);
    /* The linked static core. */
    void (*fg_options)(const rsf_native_fg_options* options);
    int (*fg_status)(rsf_native_fg_status* status);
    int (*latency_status)(rsf_native_latency_status* status);
    uint32_t (*present_backend)(void);
    int32_t (*present_switch_result)(void);
    uint64_t (*present_generation)(void);
    /* Session leaves. open sets the paths and engine identity, the log, the presentation host, the
       input window and the Vulkan surface, and prepares the renderer bridge for a new session;
       user is the one rsf_unity_package_initialize received. Non-zero on success. */
    uint32_t (*open)(void* user);
    void (*bridge_activate)(uint32_t enabled);
    uint32_t (*bridge_drained)(void);
    /* rsf_fg12_end_session for the bridge session's id; non-zero once its tokens are retired. */
    uint32_t (*end_session)(void);
    /* SR views. commands is the pass's ID3D12GraphicsCommandList. sr_default_backend resolves
       Auto through rsf_gpu_default_sr_backend for the list's adapter; zero when the device or
       adapter cannot be read. sr_create is zero when the list has no device, and otherwise stores
       rsf_sr12_create's result and context (null unless created). The rest are rsf_sr12_plan,
       rsf_sharpening_defaults, rsf_sr12_set_sharpening, rsf_sr12_evaluate and rsf_sr12_destroy. */
    uint32_t (*sr_default_backend)(void* commands, uint32_t* backend);
    uint32_t (*sr_create)(void* commands, const rsf_unity_package_view_setup* setup, int32_t* result,
        rsf_sr12** output);
    int32_t (*sr_plan)(const rsf_sr12* sr, uint32_t* width, uint32_t* height);
    void (*sharpening_defaults)(rsf_sharpening_options* output);
    int32_t (*sr_set_sharpening)(rsf_sr12* sr, const rsf_sharpening_options* options);
    int32_t (*sr_evaluate)(rsf_sr12* sr, void* commands, const rsf_game_render_pass* pass, uint32_t slot);
    void (*sr_destroy)(rsf_sr12* sr);
    /* The package log. */
    void (*log)(const char* text);
    void (*bridge_release)(void);
} rsf_unity_package_platform;

#ifdef __cplusplus
extern "C" {
#endif
/* The bodies of rsfu_configure, rsfu_settings, rsfu_status, rsfu_frame_counters and
   rsfu_temporal_ready, with the same checks and result codes. */
int rsf_unity_package_configure(const rsf_unity_package_settings* input, const rsf_unity_package_platform* platform);
int rsf_unity_package_settings_get(rsf_unity_package_settings* output);
int rsf_unity_package_status_get(rsf_unity_package_status* output, const rsf_unity_package_platform* platform);
int rsf_unity_package_frame_counters(rsf_unity_frame_counters* output, const rsf_unity_package_platform* platform);
uint32_t rsf_unity_package_temporal_ready(void);

/* Session lifecycle. initialize retires a started session once the bridge has drained and the
   Runtime has ended its session (2 while either is pending), forgets the session's plan, status
   and bridge failure, opens the new session (0 when open fails) and activates the bridge with
   the persisted reconstruction request (1). Settings persist across sessions. */
int rsf_unity_package_initialize(const rsf_unity_package_platform* platform, void* user);
/* rsfu_shutdown before its lock: deactivates the bridge; non-zero once it has drained and the
   Runtime has ended the session. */
uint32_t rsf_unity_package_quiesce(const rsf_unity_package_platform* platform);
/* Releases the views and the bridge and leaves the session stopped. */
void rsf_unity_package_retire(const rsf_unity_package_platform* platform);
/* Unity's device shutdown: deactivates the bridge and retires the session once it has drained. */
void rsf_unity_package_device_shutdown(const rsf_unity_package_platform* platform);

/* Render path, under package.cpp's mutex. */
void rsf_unity_package_current(rsf_unity_package_settings* output);
/* The host render configuration; zero when output is short. */
int rsf_unity_package_plan(rsf_game_render_config* output);
/* The render callback's SR pass (begin, role RSF_GAME_RENDER_SR) on the bridge's command list,
   after the editor preview is prepared. Applies the stored gameplay screen to a copy of the pass,
   releases every view on a reconfiguring pass, replaces a view whose extents no longer fit,
   creates a missing one (Auto resolved, a failed vendor backend retried as backend 0) and
   evaluates it in the slot bound to commands once the camera is valid and no reconfiguration is
   pending. */
void rsf_unity_package_render(void* commands, const rsf_game_render_pass* pass,
    const rsf_unity_package_platform* platform);
void rsf_unity_package_fg_result(int32_t result);
/* A renderer bridge failure reported for the rest of the session. */
void rsf_unity_package_bridge_failed(int32_t result);
#ifdef __cplusplus
}
#endif
