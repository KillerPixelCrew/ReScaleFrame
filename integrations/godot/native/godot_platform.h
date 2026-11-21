/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal boundary between the typed leaves in runtime.cpp and runtime_control.cpp and the Rust
   control owner (crate rsf-godot, native/lib.rs). Not a DLL ABI: both link into
   ReScaleFrame.Godot.Native.dll next to the same OrchestratorCore, and the rsfg_* exports in
   api.h stay C++ forwarders. native/platform.rs mirrors these layouts by hand (bindgen-cli 0.72.1
   was not available); both sides assert sizes and offsets.

   Rust owns the requested settings and their generation, the extension option schema, renderer
   admission, engine-service routing, the D3D12 session identity and paths, the status record,
   and the Vulkan session (boot state, settings, generation, status, configure admission and the
   presenter, FG and SR accounting). The leaves hold godot-cpp, D3D12, DXGI, Win32, Vulkan and
   Runtime calls and decide nothing. */
#include "api.h"
#include <stdint.h>

#define RSF_GODOT_PLATFORM_VERSION 2u

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

/* Vulkan owner facts, read under its guard. The compiled flags are build facts. */
typedef struct rsf_godot_vk_facts {
    uint32_t xess_device_ready, interop_device_ready, graph_handoff;
    uint32_t fsr4_sr_compiled, fsr4_fg_compiled, xess_fg_compiled;
    uint32_t presenter, primary_window;
    /* rsf_fgvk_backend of the live presenter; zero without one or when the query fails. */
    uint32_t presenter_backend;
    uint32_t sdk_shutdown_pending;
} rsf_godot_vk_facts;

/* rsf_latencyvk_status, as read. */
typedef struct rsf_godot_vk_latency_facts {
    uint32_t low_latency_available, report_available, pcl_message, mode, owner, reserved;
    uint64_t sleep_calls, marker_calls;
} rsf_godot_vk_latency_facts;

/* The Vulkan session as the render, presentation and capture leaves read it under the guard. */
typedef struct rsf_godot_vk_view {
    rsf_godot_settings settings;
    uint32_t sr_backend; /* the requested SR provider, Auto resolved */
    uint64_t generation, session_id;
    uint32_t fg_backend; /* the requested FG provider, Auto resolved */
    uint32_t output_width, output_height; /* the last SR pass's output */
    int32_t fg_result;
} rsf_godot_vk_view;

/* One FG pass through the presenter in rsfg_render_vulkan. */
typedef struct rsf_godot_vk_fg_pass {
    uint32_t role, screen;
    uint64_t session_id;
    uint32_t view_id, render_width, render_height;
    /* Filled by rsf_godot_vk_fg_begin: the enable rule and the requested count. */
    uint32_t enabled, generated_frames;
    /* Filled by the leaf after rsf_fgvk_enable_options and rsf_fgvk_backend. */
    int32_t options_result;
    uint32_t backend_valid, backend;
} rsf_godot_vk_fg_pass;

/* rsf_fgvk_get_status after a presenter Present. */
typedef struct rsf_godot_vk_present_facts {
    uint32_t enabled;
    int32_t result;
    uint64_t generated_callbacks, presented, tagged;
} rsf_godot_vk_present_facts;

/* How an SR pass of rsfg_render_vulkan ended. A refused pass took the spatial fallback; an
   uncounted one could not record it. identity is zero when context retirement refused the pass
   before its identity was taken. */
#define RSF_GODOT_VK_SR_UNCOUNTED 0u
#define RSF_GODOT_VK_SR_EVALUATED 1u
#define RSF_GODOT_VK_SR_REFUSED 2u
typedef struct rsf_godot_vk_sr_frame {
    uint32_t identity, outcome;
    int32_t result;
    uint32_t backend; /* the provider that produced the output, read when evaluated */
    uint32_t render_width, render_height, output_width, output_height;
    uint64_t source_frame_id;
} rsf_godot_vk_sr_frame;

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
    /* The Vulkan owner in vulkan.cpp. vulkan_supported and vulkan_latency_facts need no guard.
       vulkan_locked runs body with the owner's guard held and returns its result, or zero when it
       could not; the guard is not reentrant. Every entry after it is called only inside such a
       body and does not take the guard itself. */
    uint32_t (*vulkan_supported)(void);
    uint32_t (*vulkan_latency_facts)(rsf_godot_vk_latency_facts* facts);
    int32_t (*vulkan_locked)(int32_t (*body)(void* user), void* user);
    /* Directories, Streamline and XeSS initialization, creation resolver and the ready log. */
    int32_t (*vulkan_start)(const rsf_godot_setup* setup);
    /* Presenter disable and drain, SR context and capture retirement, XeSS shutdown and the SDK
       shutdown or its deferral to presenter retirement. Zero keeps the session. */
    int32_t (*vulkan_stop)(void);
    /* Frame dump cancel and device borrow release after the session ended. */
    void (*vulkan_release)(uint64_t session_id);
    void (*vulkan_facts)(rsf_godot_vk_facts* facts);
    int32_t (*vulkan_presenter_max)(uint32_t* count);
    /* FSR4 FG capacity on the presenter's device; zero refuses. */
    uint32_t (*vulkan_fsr4_fg_probe)(void);
    int32_t (*vulkan_latency_options)(uint32_t mode, uint32_t frame_limit_us);
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

/* Vulkan session (vulkan.rs). Boot and shutdown take the guard through vulkan_locked; the
   remaining calls are made by vulkan.cpp while it holds the guard. */
int rsf_godot_vk_booted(void);
int rsf_godot_boot_vulkan(const rsf_godot_platform* platform, const rsf_godot_setup* setup);
/* The caller found the session booted. */
int rsf_godot_shutdown_vulkan(const rsf_godot_platform* platform);
/* Fills view and returns whether the session is booted. */
int rsf_godot_vk_view_get(rsf_godot_vk_view* view);
void rsf_godot_vk_presenter_created(uint32_t ready, uint32_t backend, uint32_t max_generated);
/* Zero: the engine must drain and recreate its WSI owner. */
int rsf_godot_vk_presenter_current(uint32_t presenter_backend);
void rsf_godot_vk_presented(const rsf_godot_vk_present_facts* facts);
void rsf_godot_vk_presenter_retired(void);
void rsf_godot_vk_fg_begin(rsf_godot_vk_fg_pass* pass);
/* Nonzero: record the pass. Zero stored the refusal as the FG result. */
int rsf_godot_vk_fg_admit(const rsf_godot_vk_fg_pass* pass);
void rsf_godot_vk_fg_recorded(const rsf_godot_vk_fg_pass* pass, int32_t result);
void rsf_godot_vk_sr_frame_report(const rsf_godot_vk_sr_frame* frame);

/* Extension control (runtime_control.cpp). */
const rsf_godot_option_field* rsf_godot_option_fields(uint32_t* count);
int rsf_godot_compose_settings(const rsf_godot_platform* platform, const rsf_godot_option_value* values,
    uint32_t count, rsf_godot_settings* settings);
/* Null names are unsupported. Returns RSF_GODOT_ADMIT_* bits. */
uint32_t rsf_godot_renderer_admission(const char* driver, const char* method, uint32_t vulkan_supported);
#ifdef __cplusplus
}
#endif
