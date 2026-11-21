/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal boundary between the typed leaves in runtime.cpp and runtime_control.cpp and the Rust
   control owner (crate rsf-godot, native/lib.rs). Not a DLL ABI: both link into
   ReScaleFrame.Godot.Native.dll next to the same OrchestratorCore, and the rsfg_* exports in
   api.h stay C++ forwarders. native/platform.rs mirrors these layouts by hand (bindgen-cli 0.72.1
   was not available); both sides assert sizes and offsets.

   Rust owns the requested settings and their generation, the extension option schema, renderer
   admission, engine-service routing, the D3D12 session identity and paths, and the status record.
   The leaves hold godot-cpp, D3D12, DXGI, Win32 and Runtime calls and decide nothing. */
#include "api.h"
#include <stdint.h>

#define RSF_GODOT_PLATFORM_VERSION 1u

/* Mirror of rsf_native_fg_options without its header. */
typedef struct rsf_godot_fg_options {
    uint32_t mode, generated_frames, reflex_mode, frame_limit_us, debug;
} rsf_godot_fg_options;

/* rsf_fg12_install_explicit values chosen by the owner. The strings are owner-held and live for
   the process. The leaf adds its window admission and log trampolines. */
typedef struct rsf_godot_fg_install {
    uint32_t backend, max_generated_frames, runtime_switching, ui_mode;
    rsf_godot_fg_options options;
    uint32_t depth_inverted, depth_infinite;
    float units_to_meters;
    uint32_t engine_type;
    const char* fidelityfx_directory;
    const char* xess_directory;
    const char* streamline_directory;
    const char* engine_version;
    const char* project_id;
} rsf_godot_fg_install;

/* D3D12 session paths for SR context creation. Owner-held, valid for the process. */
typedef struct rsf_godot_paths {
    const char* runtime_directory;
    const char* streamline_directory;
    const char* streamline_interposer;
    const char* fidelityfx_directory;
    const char* xess_directory;
    const char* engine_version;
    const char* project_id;
} rsf_godot_paths;

/* Shared input counters, typed as the status fields they fill. */
typedef struct rsf_godot_input_facts {
    uint64_t keyboard, mouse, controller, pings, substitutes, order_violations, derived_boundaries;
} rsf_godot_input_facts;

typedef struct rsf_godot_window_facts {
    uint32_t foreground_process_id, current_process_id;
    uint32_t input_window_visible, reserved;
} rsf_godot_window_facts;

/* rsf_fg12_status plus the shared presentation provider, typed as the status fields. */
typedef struct rsf_godot_fg_facts {
    uint32_t available, active, reason, max_generated_frames, generated_frames;
    uint32_t latency_available, latency_mode, statistics_flags;
    int32_t last_result, switch_result;
    uint32_t backend, reserved;
    uint64_t tagged_frames, total_presented, generated_presented, source_frame_id, generated_callbacks;
    uint64_t provider_generation;
} rsf_godot_fg_facts;

/* rsf_native_latency_status, typed as the status fields. */
typedef struct rsf_godot_latency_facts {
    uint32_t pcl_available, report_available, owner, reserved;
    uint64_t paced_frames, marker_calls;
    rsf_godot_input_facts inputs;
} rsf_godot_latency_facts;

/* One evaluated D3D12 SR pass, reported after rsf_sr12_evaluate. */
typedef struct rsf_godot_sr_frame {
    uint32_t requested_sr, context_backend;
    int32_t context_result, evaluated;
    uint32_t render_width, render_height, output_width, output_height;
} rsf_godot_sr_frame;

/* Process-lived table from runtime.cpp. Every entry is present and catches its own exceptions. */
typedef struct rsf_godot_platform {
    uint32_t struct_size, version;
    uint64_t (*tick_count)(void);
    /* Creates the D3D12 frame capture owner; zero refuses the boot. */
    uint32_t (*prepare_capture)(void);
    int32_t (*fg_install)(const rsf_godot_fg_install* install);
    void (*fg_options)(const rsf_godot_fg_options* options);
    /* Header-inline Runtime validators and defaults. */
    uint32_t (*dlss_preset_valid)(uint32_t preset);
    uint32_t (*sharpening_valid)(const rsf_sharpening_options* options);
    void (*sharpening_defaults)(rsf_sharpening_options* options);
    uint32_t (*fg_backend_known)(uint32_t backend);
    /* The Vulkan owner in vulkan.cpp. */
    uint32_t (*vulkan_booted)(void);
    int32_t (*vulkan_configure)(const rsf_godot_settings* settings);
    int32_t (*vulkan_settings)(rsf_godot_settings* settings);
    int32_t (*vulkan_status)(rsf_godot_status* status);
    /* Status facts. latency_facts returns zero when the Runtime has no latency status. */
    void (*cpu_counters)(rsf_godot_input_facts* facts);
    void (*window_facts)(rsf_godot_window_facts* facts);
    void (*fg_facts)(rsf_godot_fg_facts* facts);
    uint32_t (*latency_facts)(rsf_godot_latency_facts* facts);
} rsf_godot_platform;

/* Extension option schema. The leaf reads each named Dictionary entry once, with the conversion
   the field names; the owner validates and composes the settings. */
#define RSF_GODOT_OPTION_COUNT 21u
#define RSF_GODOT_OPTION_INTEGER 0u
#define RSF_GODOT_OPTION_REAL 1u
#define RSF_GODOT_VARIANT_OTHER 0u
#define RSF_GODOT_VARIANT_INT 1u
#define RSF_GODOT_VARIANT_BOOL 2u
typedef struct rsf_godot_option_field {
    const char* name;
    uint32_t conversion;
    uint32_t reserved;
} rsf_godot_option_field;
typedef struct rsf_godot_option_value {
    uint32_t present, variant;
    int64_t integer;
    double real;
} rsf_godot_option_value;

#define RSF_GODOT_ADMIT_DRIVER 1u
#define RSF_GODOT_ADMIT_RENDERER 2u

#ifdef __cplusplus
extern "C" {
#endif
/* runtime.cpp: the table runtime_control.cpp passes to the owner. */
const rsf_godot_platform* rsf_godot_platform_leaves(void);

/* Rust owner. platform is the process-lived table above; the first accepted one is kept. */
int rsf_godot_attach_engine(const rsf_godot_engine_services* services);
int rsf_godot_configure_engine(const rsf_godot_platform* platform, const rsf_godot_settings* settings,
    uint64_t viewport_id);
void rsf_godot_reset_history(const rsf_godot_platform* platform);
int rsf_godot_boot(const rsf_godot_platform* platform, const rsf_godot_setup* setup);
int rsf_godot_configure(const rsf_godot_platform* platform, const rsf_godot_settings* settings);
int rsf_godot_settings_get(const rsf_godot_platform* platform, rsf_godot_settings* settings);
int rsf_godot_status_get(const rsf_godot_platform* platform, rsf_godot_status* status);
/* rsf_godot_status_get succeeded with a nonzero session. */
int rsf_godot_status_ready(const rsf_godot_platform* platform, rsf_godot_status* status);
void rsf_godot_log(const char* message);

/* D3D12 session state read and reported by the SR and presentation leaves in runtime.cpp. */
int rsf_godot_booted(void);
uint64_t rsf_godot_session_id(void);
int rsf_godot_paths_get(rsf_godot_paths* paths);
void rsf_godot_settings_snapshot(rsf_godot_settings* settings, uint64_t* generation);
void rsf_godot_sr_frame_report(const rsf_godot_sr_frame* frame);
/* Present without an SR pass for the current render frame. */
void rsf_godot_sr_missed(void);

/* Extension control (runtime_control.cpp). */
const rsf_godot_option_field* rsf_godot_option_fields(uint32_t* count);
int rsf_godot_compose_settings(const rsf_godot_platform* platform, const rsf_godot_option_value* values,
    uint32_t count, rsf_godot_settings* settings);
/* Null names are unsupported. Returns RSF_GODOT_ADMIT_* bits. */
uint32_t rsf_godot_renderer_admission(const char* driver, const char* method, uint32_t vulkan_supported);
#ifdef __cplusplus
}
#endif
