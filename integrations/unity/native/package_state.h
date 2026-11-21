/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal boundary between the Unity package's native leaves (package.cpp) and their Rust state
   owner (games/unity-mono/src/package_state.rs), which lives in the rsf-unity-mono archive the
   package already links. Not a DLL ABI. tools/generate-rust-bindings.py generates
   games/unity-mono/src/package_abi.rs from this header.

   The settings, status and frame-counter structures are the package's managed contract
   (Runtime/Native.cs, rsfu_*) and keep their layout. Rust owns the stored settings and their
   validation, the status snapshot, the render plan, temporal readiness, the pending
   reconfiguration, the session lifecycle with its result codes, the D3D12 SR views of the
   render callback (package_views.rs): one Runtime SR context per Unity view key, its backend
   choice and fallback, its replacement on new extents or reconfiguration, and the recording
   slots bound to the bridge's command lists, and the presentation (package_state/presentation.rs):
   the package identity (SDK root and directories, Unity version and project id), the one
   generation install and the window pass's provider request. package.cpp keeps the Unity, DXGI,
   Vulkan and preview work, the render callback's routing, the window guard, the input window and
   the log, and lends the typed SDK and Runtime calls through rsf_unity_package_platform.
   package.cpp's mutex still serializes the exports and the SR pass; nothing here takes it. Rust
   never holds its state lock while it calls a leaf; its view lock is held across the SR leaves
   and its presentation lock across the install leaf, none of which re-enter, and both are always
   taken before the state lock. */
#include <rescaleframe/game_renderer.h>
#include <rescaleframe/sharpening_settings.h>
#include <stdint.h>
#include <wchar.h>

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
/* Defined by rescaleframe/native_fg_d3d12.h; the generation install. */
typedef struct rsf_fg12_setup rsf_fg12_setup;

/* The Runtime SR context a view asks for: its resolved backend (0 is the spatial or native path),
   the stored quality and DLSS model, the pass camera's extents and depth convention, and the
   package identity as UTF-8, borrowed for the call: the SDK root, its streamline, fidelityfx and
   xess directories, the Streamline interposer, Unity's version and the project id. package.cpp
   adds the device, Streamline host and log. */
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

/* package.cpp's typed leaves, all present. No leaf calls back into a rsf_unity_package_* entry
   except open, which may call rsf_unity_package_install; none may throw. */
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
    /* Session leaves. open opens the per-process log, installs generation in the Editor, hooks
       the input window, creates the Vulkan surface and prepares the renderer bridge for a new
       session, after rsf_unity_package_initialize stored the session's identity. Non-zero on
       success. */
    uint32_t (*open)(void);
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
    /* Generation. accept_window is the package's window guard (the Editor preview, the Vulkan
       surface or this process's Unity player window) and backend_log the package log in the
       Runtime's form; both go into the install. fg_install is rsf_fg12_install, present_has_owner
       and present_request are rsf_d3d11_present_has_owner and rsf_d3d11_present_request.
       fg_default_backend resolves Auto through rsf_fg_default_backend for the adapter of Unity's
       D3D12 device; zero when the device or adapter cannot be read. */
    int (*accept_window)(void* user, void* hwnd);
    void (*backend_log)(void* user, const char* message);
    int32_t (*fg_install)(const rsf_fg12_setup* setup);
    uint32_t (*present_has_owner)(void);
    int32_t (*present_request)(uint32_t backend);
    uint32_t (*fg_default_backend)(uint32_t* backend);
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
   and bridge failure, stores rsfu_initialize's identity (the UTF-8 SDK root, Unity version and
   project id; 0 when one is null), opens the new session (0 when open fails) and activates the
   bridge with the persisted reconstruction request (1). Settings persist across sessions. */
int rsf_unity_package_initialize(const char* runtime, const char* engine, const char* project,
    const rsf_unity_package_platform* platform);
/* rsfu_shutdown before its lock: deactivates the bridge; non-zero once it has drained and the
   Runtime has ended the session. */
uint32_t rsf_unity_package_quiesce(const rsf_unity_package_platform* platform);
/* Releases the views and the bridge and leaves the session stopped. */
void rsf_unity_package_retire(const rsf_unity_package_platform* platform);
/* Unity's device shutdown: deactivates the bridge and retires the session once it has drained. */
void rsf_unity_package_device_shutdown(const rsf_unity_package_platform* platform);

/* Render path, under package.cpp's mutex. */
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

/* Presentation, outside package.cpp's mutex except where a locked export calls it. load reads
   the identity before any session from [UnitySR] of `ini` (NUL-terminated UTF-16): Runtime,
   resolved like every [UnitySR] path, and EngineVersion and ProjectId. */
void rsf_unity_package_load(const wchar_t* ini);
/* Installs the shared D3D12 generation once per process, with the Auto provider, generation off
   and runtime switching, when the identity has a root, a version and a project id; a refusal is
   retried by the next call. Its result is the status's generation result. The player installs
   at load, the Editor from open. */
void rsf_unity_package_install(const rsf_unity_package_platform* platform);
/* The window pass, before its presentation: while the presentation host has an owner, asks it
   for the stored generation provider unless that one is active, and records the request's
   result. Auto is resolved for Unity's adapter and kept once it names a provider. */
void rsf_unity_package_request_generation(const rsf_unity_package_platform* platform);
/* A renderer bridge failure reported for the rest of the session. */
void rsf_unity_package_bridge_failed(int32_t result);
#ifdef __cplusplus
}
#endif
