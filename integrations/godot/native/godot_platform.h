/* SPDX-License-Identifier: MIT */
#pragma once
#include "extension_abi.h"
/* Internal boundary between the leaves and the Rust control owner (crate rsf-godot,
   native/lib.rs). Not a DLL ABI: everything links into ReScaleFrame.Godot.Native.dll next to the
   same OrchestratorCore, and Rust exports the rsfg_* entries of api.h through exports.def.
   native/platform.rs mirrors these layouts by hand (bindgen-cli 0.72.1 was not available);
   runtime_platform.c and platform.rs both assert sizes and offsets.

   Rust owns the requested settings and their generation, the extension option schema, renderer
   admission, engine-service routing, the D3D12 session identity and paths, the status record,
   the D3D12 SR context plan and submission ledger (d3d12.rs), the Vulkan session (boot state,
   settings, generation, status, configure admission, the providers Auto may choose from and their
   resolution through the Runtime's policy, the presenter, FG and SR accounting, the
   primary window, the SR context owner in vulkan/sr.rs and the device creation owner in
   vulkan/creation.rs), and the input window owner (window.rs: admission, window procedure, input
   session, CPU event and ping routing, and the present-marker admission). The leaves hold
   Godot, D3D12, DXGI, Win32, Vulkan and Runtime calls and decide nothing. The creation leaves
   use a separate Rust callback table at each creation call. The compositor effect's leaves are
   another (rsf_godot_effect_leaves, gdextension/effect.rs), lent with each callback. */
#include "api.h"
#include <rescaleframe/native_cpu_service.h>
#include <stdint.h>

#define RSF_GODOT_PLATFORM_VERSION 9u

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

/* The owner process and class of a candidate input window, as read. process_id is zero for an
   invalid window; class_length is GetClassNameW's count into class_name, zero on failure. */
typedef struct rsf_godot_window_identity {
    uint32_t process_id, current_process_id;
    uint32_t class_length, reserved;
    uint16_t class_name[64];
} rsf_godot_window_identity;

/* rsf_fg12_status plus the shared presentation provider, typed as the status fields. */
typedef struct rsf_godot_fg_facts {
    uint32_t available, active, reason, max_generated_frames, generated_frames;
    uint32_t statistics_flags;
    int32_t last_result, switch_result;
    uint32_t backend, reserved;
    uint64_t tagged_frames, total_presented, generated_presented, source_frame_id, generated_callbacks;
    uint64_t provider_generation;
} rsf_godot_fg_facts;

/* rsf_native_latency_status, typed as the status fields. */
typedef struct rsf_godot_latency_facts {
    uint32_t available, effective_mode;
    uint32_t pcl_available, report_available, owner, reserved;
    uint64_t paced_frames, marker_calls;
    rsf_godot_input_facts inputs;
} rsf_godot_latency_facts;

/* One D3D12 SR context the owner planned: the provider (Auto already resolved by the Runtime,
   spatial mapped to the normalising context), quality, preset and extents. The leaf adds the
   bound device, the presentation host and the log. */
typedef struct rsf_godot_sr_create {
    uint32_t backend, quality, dlss_preset;
    uint32_t output_width, output_height, render_width, render_height;
    uint32_t reserved;
    rsf_godot_paths paths;
} rsf_godot_sr_create;

/* rsf_sr12_create's result and handle, and rsf_sr12_plan's extent when a context exists. The
   owner holds the handle; desired_* arrive holding the previous plan and stay when none is made. */
typedef struct rsf_godot_sr_context {
    void* handle;
    int32_t result;
    uint32_t desired_width, desired_height, reserved;
} rsf_godot_sr_context;

/* One rsf_sr12_evaluate: the command slot, the gated screen and the sharpening snapshot. */
typedef struct rsf_godot_sr_evaluate {
    uint32_t slot, screen;
    rsf_sharpening_options sharpening;
} rsf_godot_sr_evaluate;

/* Vulkan owner facts, read under its guard. The compiled flags are build facts. */
typedef struct rsf_godot_vk_facts {
    /* Formerly xess_device_ready and interop_device_ready; the creation owner (vulkan/creation.rs)
       keeps both now. Zero. */
    uint32_t reserved_creation[2];
    uint32_t graph_handoff;
    uint32_t fsr4_sr_compiled, fsr4_fg_compiled, xess_fg_compiled;
    uint32_t presenter;
    /* rsf_slvk_initialize with DLSS SR and FG succeeded at this session's start: the SDK
       availability Auto requires before it may choose DLSS. Formerly reserved. */
    uint32_t streamline_ready;
    /* rsf_fgvk_backend of the live presenter; zero without one or when the query fails. */
    uint32_t presenter_backend;
    uint32_t sdk_shutdown_pending;
} rsf_godot_vk_facts;

/* rsf_latencyvk_status, as read. */
typedef struct rsf_godot_vk_latency_facts {
    uint32_t low_latency_available, report_available, pcl_message, mode, owner, reserved;
    uint64_t sleep_calls, marker_calls;
} rsf_godot_vk_latency_facts;

/* The provider and quality of one Vulkan SR context. The leaf adds the packet's device, procs and
   camera depth and motion facts and the provider's runtime directory. */
typedef struct rsf_godot_vk_sr_create {
    uint32_t backend, quality, preset;
} rsf_godot_vk_sr_create;

/* The created context, kept by the owner even when result is not RSF_BACKEND_OK. */
typedef struct rsf_godot_vk_sr_context {
    void* handle;
    int32_t result;
    uint32_t reserved;
} rsf_godot_vk_sr_context;

/* The compositor effect boundary. The effect owner (effect.rs) keeps each ReScaleFrameEffect's
   published snapshot, its callback count, the pending capture request and the last motion capture,
   the scene buffer admission, the capture admission and the motion classification, and the probe's
   published stage, reason and inactive pipeline claims. gdextension/effect.rs keeps the
   CompositorEffect class, its registration and callback contract, the RenderSceneBuffersRD,
   RenderSceneData and RenderingDevice reads, the readback compute calls and the status
   Dictionary conversion. call is the render callback's private record. These leaves take no
   guard and call no other owner. */
#define RSF_GODOT_EFFECT_VERSION 2u
/* RenderingDevice::DATA_FORMAT_R16G16_SFLOAT, the engine's velocity format and the only one the
   readback is interpreted for. gdextension/effect.rs asserts the value. */
#define RSF_GODOT_EFFECT_MOTION_FORMAT 82

/* The scene buffer facts of one POST_TRANSPARENT callback. Matrices are the scene data's real_t
   values widened to double: camera_transform is the basis rows then the origin, and
   jittered_projection the four columns. The three valid flags are texture_is_valid of layer 0. */
typedef struct rsf_godot_effect_frame {
    uint64_t render_target;
    uint32_t views;
    int32_t internal_width, internal_height, output_width, output_height;
    uint32_t color_valid, depth_valid, motion_valid;
    uint32_t reserved[2];
    double camera_transform[12];
    double jittered_projection[16];
} rsf_godot_effect_frame;

/* Read only for admitted buffers: the three layer formats (RenderingDevice::DataFormat), and
   whether the logical device, the command queue and all three native textures have handles. */
typedef struct rsf_godot_effect_resources {
    int32_t color_format, depth_format, motion_format;
    uint32_t native_device, native_queue, native_textures;
} rsf_godot_effect_resources;

/* An effect's published state, copied under the owner's lock. stage, reason and snapshot_scope are
   owner-held static strings. A field the last callback did not reach keeps its default: zero, the
   identity matrices and format -1. */
typedef struct rsf_godot_effect_status {
    uint32_t struct_size, version;
    uint64_t callbacks;
    rsf_godot_effect_frame frame;
    rsf_godot_effect_resources resources;
    uint32_t buffers_available;
    uint32_t capture_succeeded;
    uint64_t captures, capture_callback, capture_pixels, capture_moving_pixels;
    uint64_t capture_nonfinite_pixels, capture_unwritten_pixels;
    double capture_max_abs_uv;
    const char* stage;
    const char* reason;
    const char* snapshot_scope;
    uint32_t rendering_ready, sr_active, fg_active, latency_active;
} rsf_godot_effect_status;

/* Process-lived table from gdextension/effect.rs, lent with each render callback. Every entry is
   present; a completion of zero means a Godot call was unavailable or refused. */
typedef struct rsf_godot_effect_readback {
    const char* shader;
    uint32_t bytes, groups_x, groups_y;
} rsf_godot_effect_readback;

typedef struct rsf_godot_effect_leaves {
    uint32_t struct_size, version;
    /* The scene buffers, scene data and the RenderingServer's RenderingDevice, then the frame facts
       and the layer 0 color, depth and velocity textures kept in call. Zero when any of the three
       is missing; frame is then untouched. */
    uint32_t (*frame)(void* call, rsf_godot_effect_frame* frame);
    /* The layer formats and native handle facts of the textures frame kept. */
    uint32_t (*resources)(void* call, rsf_godot_effect_resources* resources);
    /* Converts the velocity texture through the render graph into an owned storage buffer of
       internal_width * internal_height vec2 and reads it back (a GPU stall). The bytes stay in call
       until the callback ends; empty when the shader, pipeline or buffer was refused. */
    uint32_t (*read_motion)(void* call, const rsf_godot_effect_readback* plan, const uint8_t** data, uint64_t* size);
} rsf_godot_effect_leaves;

typedef struct rsf_godot_effect rsf_godot_effect;

/* rsf_godot_platform.vulkan_sr_log messages. Each leaf writes nothing without a session log. */
#define RSF_GODOT_VK_LOG_REJECTED 1u       /* provider result and spatial fallback result */
#define RSF_GODOT_VK_LOG_FSR4_DISPATCH 2u  /* the last FSR4 evaluate's code and continuation */
#define RSF_GODOT_VK_LOG_INPUT_REFUSAL 3u  /* the last evaluated frame, its plan and record */

/* Process-lived table from host.rs. Every entry is present. */
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
    /* The Rust Vulkan bridge. vulkan_supported and vulkan_latency_facts need no guard.
       vulkan_locked runs body with the owner's guard held and returns its result, or zero when it
       could not; the guard is not reentrant. Every entry after it is called only inside such a
       body and does not take the guard itself. */
    uint32_t (*vulkan_supported)(void);
    uint32_t (*vulkan_latency_facts)(rsf_godot_vk_latency_facts* facts);
    int32_t (*vulkan_locked)(int32_t (*body)(void* user), void* user);
    /* Directories, Streamline and XeSS initialization, creation resolver and the ready log. */
    int32_t (*vulkan_start)(const rsf_godot_setup* setup);
    /* After the owner drained the presenter and retired the SR contexts: capture retirement,
       XeSS shutdown and the SDK shutdown or its deferral to presenter retirement. Zero keeps the
       session. */
    int32_t (*vulkan_stop)(void);
    /* Frame dump cancel and runtime directory release after the session ended. */
    void (*vulkan_release)(uint64_t session_id);
    void (*vulkan_facts)(rsf_godot_vk_facts* facts);
    int32_t (*vulkan_presenter_max)(uint32_t* count);
    /* FSR4 FG capacity on the presenter's device; zero refuses. */
    uint32_t (*vulkan_fsr4_fg_probe)(void);
    int32_t (*vulkan_latency_options)(uint32_t mode, uint32_t frame_limit_us);
    /* Status facts. latency_facts returns zero when the Runtime has no latency status. */
    void (*cpu_counters)(rsf_godot_input_facts* facts);
    /* The foreground process and whether window, the admitted input window or null, is visible. */
    void (*window_facts)(void* window, rsf_godot_window_facts* facts);
    void (*fg_facts)(rsf_godot_fg_facts* facts);
    uint32_t (*latency_facts)(rsf_godot_latency_facts* facts);
    /* The D3D12 SR context and submission leaves (d3d12.rs), called on Godot's render thread.
       A uint32_t completion of zero means an exception ended the leaf; the owner then refuses
       the call as the export's catch did. */
    /* Keeps the engine's first device and queue and creates the fence; zero when the fence
       failed, which keeps the borrow as before. */
    uint32_t (*d3d12_bind)(void* device, void* queue);
    /* Nonzero when the fence reached value within 15 s, or value is zero, or no fence exists. */
    uint32_t (*d3d12_wait)(uint64_t value);
    /* Queue Signal of value; zero without a fence or on failure. */
    uint32_t (*d3d12_signal)(uint64_t value);
    /* The Runtime's Auto SR and FG provider for the bound adapter. */
    uint32_t (*sr_default_backend)(void);
    uint32_t (*fg_default_backend)(void);
    /* Nonzero when the presentation provider has an owner; writes its backend. */
    uint32_t (*present_owner)(uint32_t* backend);
    void (*present_request)(uint32_t backend);
    uint32_t (*sr_create)(const rsf_godot_sr_create* request, rsf_godot_sr_context* context);
    void (*sr_destroy)(void* context);
    /* Sharpening snapshot, then rsf_sr12_evaluate of a copy of pass with request->screen. */
    uint32_t (*sr_evaluate)(void* context, void* commands, const rsf_game_render_pass* pass,
        const rsf_godot_sr_evaluate* request, int32_t* result);
    /* Logs the refused pass, its resources and the planned extent. */
    void (*sr_refusal_log)(const rsf_game_render_pass* pass, uint32_t desired_width, uint32_t desired_height);
    /* The "native-sr-begin" capture event when the capture wants the frame. */
    uint32_t (*capture_sr_begin)(const rsf_game_render_pass* pass);
    /* Capture flush of slot, the HUD-less colour record and rsf_fg12_hudless. */
    uint32_t (*final_scene)(void* commands, const rsf_game_render_pass* pass, uint32_t slot);
    void (*fg_submitted)(uint64_t session_id, uint64_t frame_id);
    /* rsf_fg12_render_begin and its "render-submit-begin" capture event. */
    uint32_t (*render_begin)(uint64_t session_id, uint64_t frame_id);
    void (*present_window)(const rsf_game_render_pass* pass);
    uint32_t (*fg_end_session)(uint64_t session_id);
    /* Capture flush, context destroy (null accepted) and the session's frame dump cancel. */
    uint32_t (*d3d12_release)(void* context, uint64_t session_id);
    /* The Vulkan SR context leaves (vulkan/sr.rs), called only while the Rust graphics guard is held:
       inside a vulkan_locked body or from the Vulkan SR pass. None takes the guard. */
    /* rsf_fgvk_drain of the live presenter, after rsf_fgvk_enable(0) when disable is nonzero;
       RSF_BACKEND_OK without a presenter. */
    int32_t (*vulkan_presenter_drain)(uint32_t disable);
    /* vkDeviceWaitIdle through device_proc; zero without the entry point or on failure. */
    uint32_t (*vulkan_device_idle)(void* device, rsf_godot_vk_proc device_proc);
    int32_t (*vulkan_sr_destroy)(void* context);
    uint32_t (*vulkan_sr_create)(void* call, const rsf_godot_vk_sr_create* request, rsf_godot_vk_sr_context* created);
    uint32_t (*vulkan_sr_plan)(void* call, void* context, uint32_t* width, uint32_t* height);
    uint32_t (*vulkan_sr_record)(void* call, void* context, const rsf_sharpening_options* sharpening,
        int32_t* provider, int32_t* output, void** next);
    /* One RSF_GODOT_VK_LOG_* message. */
    uint32_t (*vulkan_sr_log)(void* call, uint32_t kind, int32_t provider_result, int32_t fallback_result);
    /* rsf_input_dump_result of the call's record for the "sr" stage. */
    uint32_t (*vulkan_sr_dump_result)(void* call, uint32_t backend, int32_t result);
    /* The window and input leaves (window.rs). Windows are handles passed through; a procedure is
       a WNDPROC as GWLP_WNDPROC stores it. window_forward runs on the window's thread inside the
       owner's procedure and may re-enter it. */
    /* GetWindowThreadProcessId, GetCurrentProcessId and GetClassNameW into 64 characters. */
    void (*window_identity)(void* window, rsf_godot_window_identity* identity);
    /* GetWindowLongPtrW(GWLP_WNDPROC); zero on failure. */
    intptr_t (*window_procedure_get)(void* window);
    /* SetWindowLongPtrW(GWLP_WNDPROC): the replaced procedure, zero on failure. */
    intptr_t (*window_procedure_set)(void* window, intptr_t procedure);
    /* CallWindowProcW of previous, or DefWindowProcW when previous is zero. */
    intptr_t (*window_forward)(intptr_t previous, void* window, uint32_t message, uintptr_t wparam,
        intptr_t lparam);
    /* QueryPerformanceCounter and QueryPerformanceFrequency. */
    void (*query_counter)(uint64_t* counter, uint64_t* frequency);
    /* Endpoints supplied by the Runtime graphics session at setup. */
    const rsf_cpu_session_service* (*d3d12_cpu_service)(void);
    const rsf_cpu_session_service* (*vulkan_cpu_service)(void);
    /* rsf_latencyvk_present. */
    void (*vulkan_present_marker)(uint32_t begin);
    /* Version 6. The Runtime's Auto policy for an adapter and the providers a host can serve (bit n
       = provider n): rsf_gpu_select_sr_backend, and for FG rsf_fg_default_backend's preference
       through the Runtime's private rsf_fg_select_auto_backend (fg_auto_choice.h). */
    uint32_t (*sr_select_backend)(uint32_t vendor, uint32_t software, uint32_t choices);
    uint32_t (*fg_select_backend)(uint32_t vendor, uint32_t software, uint32_t choices);
} rsf_godot_platform;

/* Extension option schema. The leaf reads each named Dictionary entry once, with the conversion
   the field names; the owner validates and composes the settings. */
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
/* Rust owner. The first accepted process-lived platform table is kept. */
int rsf_godot_boot(const rsf_godot_platform* platform, const rsf_godot_setup* setup);
int rsf_godot_configure(const rsf_godot_platform* platform, const rsf_godot_settings* settings);
void rsf_godot_log(const char* message);

/* D3D12 session state read by the engine entry points. */
int rsf_godot_booted(void);

/* D3D12 render, render-begin and shutdown operations reached from the public entry points. */
int rsf_godot_d3d12_render(const rsf_godot_platform* platform, void* device, void* queue, void* commands,
    const rsf_game_render_pass* pass);
void rsf_godot_d3d12_render_begin(const rsf_godot_platform* platform, uint64_t source_frame_id);
int rsf_godot_d3d12_shutdown(const rsf_godot_platform* platform);

/* Vulkan session. Boot and shutdown take the guard through vulkan_locked. */
int rsf_godot_vk_booted(void);
int rsf_godot_boot_vulkan(const rsf_godot_platform* platform, const rsf_godot_setup* setup);
/* The caller found the session booted. */
int rsf_godot_shutdown_vulkan(const rsf_godot_platform* platform);
/* Compositor effect owner (effect.rs). Each ReScaleFrameEffect instance (gdextension/effect.rs)
   owns one directly; these entries expose the same owner to the Rust fixtures. request_capture and
   status run on the caller's thread, render on the render thread after the callback type check. */
rsf_godot_effect* rsf_godot_effect_create(void);
void rsf_godot_effect_destroy(rsf_godot_effect* effect);
void rsf_godot_effect_request_capture(rsf_godot_effect* effect);
void rsf_godot_effect_render(rsf_godot_effect* effect, const rsf_godot_effect_leaves* leaves, void* call);
/* Synchronous capture recording with borrowed SDK objects and a four-entry validity array. */
uint32_t rsf_godot_effect_record_motion(void* call, const rsf_godot_effect_readback* plan,
    const rsf_godot_motion_calls* calls);
void rsf_godot_effect_properties(const rsf_godot_effect* effect, const char* version,
    void* user, rsf_godot_property_emit emit);
/* Nonzero when the record's header matched and it was filled. */
int rsf_godot_effect_status_get(const rsf_godot_effect* effect, rsf_godot_effect_status* status);

/* Input window owner (window.rs). */
/* The Runtime's accept_window question: nonzero for the admitted window, the first window of this
   process with the Engine class. */
int rsf_godot_accept_window(const rsf_godot_platform* platform, void* window);
/* The window part of rsfg_shutdown: the checked procedure restore and the end of the admission
   and input session. */
void rsf_godot_window_release(const rsf_godot_platform* platform);
/* rsfg_vk_present_marker: nonzero when the admitted window's marker was reported. */
int rsf_godot_execution(const rsf_game_render_pass* pass, uint32_t begin);
int rsf_godot_vk_present_marker(const rsf_godot_platform* platform, uint64_t window, uint32_t begin);

/* Extension control, called by ReScaleFrameRuntime (native/gdextension/runtime.rs). */
int rsf_godot_extension_configure(uint32_t admitted,
    uint64_t viewport, const void* dictionary,
    uint32_t (*read)(const void*, const rsf_godot_option_field*, rsf_godot_option_value*));
/* Emits script status fields into a borrowed scripting Dictionary. */
void rsf_godot_extension_status(uint32_t admitted, void* dictionary, rsf_godot_property_emit emit);
/* The CMake project version, from runtime_platform.c. */
const char* rsf_godot_version(void);
#ifdef __cplusplus
}
#endif
