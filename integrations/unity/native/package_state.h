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
   slots bound to the bridge's command lists, the presentation (package_state/presentation.rs):
   the package identity (SDK root and directories, Unity version and project id), the one
   generation install and the window pass's provider request, and the session services
   (package_state/services.rs): the host services and bridge API of each session, the log file,
   the input window's registration and retirement, and the routing of CPU events, window messages
   and rsfu_input into the Runtime, and the routing (package_state/routing.rs): the render
   callback's window, final-scene, SR and submitted passes with the Editor preview's admission and
   diagnostic budget, the generation window guard and Unity's device lifecycle. package.cpp keeps
   the Unity, DXGI, Vulkan and preview work and lends the typed Unity, Win32, SDK and Runtime
   calls through rsf_unity_package_platform. package.cpp's mutex still serializes the exports,
   Unity device events and the SR pass; nothing here takes it. Rust never holds its state lock while it calls
   a leaf; its view lock is held across the SR leaves and its presentation lock across the install
   leaf, none of which re-enter, and both are always taken before the state lock. Its log lock is
   held across the log leaves only and its input-window lock across the window leaves only; the
   message and CPU paths take neither. */
#include <rescaleframe/game_renderer.h>
#include <rescaleframe/sharpening_settings.h>
#include <rescaleframe/unity_bridge.h>
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
   except the log, render, bridge and Runtime leaves, which may reach rsf_unity_package_log; none
   may throw. Rust adopts the constant table at plugin load. */
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
    /* Session leaves. Closing deactivates the bridge and verifies its submitted work. A
       successful close holds admission and the Vulkan presentation lease until finish. */
    void (*bridge_activate)(uint32_t enabled);
    uint32_t (*bridge_try_close)(void);
    /* rsf_fg12_end_session for a session's id; non-zero once its tokens are retired. */
    uint32_t (*end_session)(uint64_t session);
    /* SR views. sr_quality_supported checks an explicit request. sr_create borrows the device
       from commands, calls the Runtime's planned create and writes the original provider result.
       On success it returns a context and updates the setup's backend and render dimensions. */
    uint32_t (*sr_quality_supported)(uint32_t backend, uint32_t quality);
    uint32_t (*sr_create)(void* commands, rsf_unity_package_view_setup* setup, int32_t* result,
        rsf_sr12** output);
    void (*sharpening_defaults)(rsf_sharpening_options* output);
    int32_t (*sr_set_sharpening)(rsf_sr12* sr, const rsf_sharpening_options* options);
    int32_t (*sr_evaluate)(rsf_sr12* sr, void* commands, const rsf_game_render_pass* pass, uint32_t slot);
    void (*sr_destroy)(rsf_sr12* sr);
    /* Commit destroys the verified bridge and keeps it closed. On refusal release only the
       presentation lease, retaining closed bridge admission and all resources for retry. */
    void (*bridge_finish_retirement)(uint32_t commit);
    /* Generation. accept_window is the package's window guard (the Editor preview, the Vulkan
       surface or this process's Unity player window) and backend_log the package log in the
       Runtime's form (rsf_unity_package_log); both go into the install. fg_install is
       rsf_fg12_install, present_has_owner and present_request are rsf_d3d11_present_has_owner
       and rsf_d3d11_present_request. fg_default_backend resolves Auto through
       rsf_fg_default_backend for the adapter of Unity's D3D12 device; zero when the device or
       adapter cannot be read. */
    int (*accept_window)(void* user, void* hwnd);
    void (*backend_log)(void* user, const char* message);
    int32_t (*fg_install)(const rsf_fg12_setup* setup);
    uint32_t (*present_has_owner)(void);
    int32_t (*present_request)(uint32_t backend);
    uint32_t (*fg_default_backend)(uint32_t* backend);
    /* Session services. bridge_open resumes the bridge after committed retirement and opens the
       player's Vulkan surface on the window thread, if applicable. session_seed is
       GetTickCount64. render_pass and render_config are package.cpp's render callback routing
       and its locked render plan, handed to the bridge unchanged. bridge_prepare is
       rsf_unity_bridge_prepare, which writes the managed API into api; non-zero on success. */
    uint32_t (*bridge_open)(uint32_t editor);
    uint64_t (*session_seed)(void);
    rsf_game_render_pass_fn render_pass;
    rsf_game_render_config_fn render_config;
    uint32_t (*bridge_prepare)(const rsf_game_host_services* services, rsf_unity_native_api* api);
    /* The log file. local_app_data copies %LOCALAPPDATA% into output and returns its length, zero
       when it is unset, empty or does not fit. log_open opens path for appending, shared for
       reading and writing, created when missing; null on failure. log_write writes text and a
       line break to the debugger output and, when file is not null, text and CRLF to file. */
    uint32_t (*local_app_data)(wchar_t* output, uint32_t capacity);
    void (*create_directory)(const wchar_t* path);
    void* (*log_open)(const wchar_t* path);
    void (*log_write)(void* file, const char* text);
    void (*log_close)(void* file);
    /* The input window. player_window is this process's first UnityWndClass top-level window,
       null when there is none; preview_parent is the Editor preview's parent window, null before
       it exists. subclass returns nonzero on attachment. restore disables reporting and attempts
       owner-thread detachment, retaining forwarding state and a module reference on refusal.
       raw_input_type reads a WM_INPUT header's dwType; zero when the header cannot be read. */
    void* (*player_window)(void);
    void* (*preview_parent)(void);
    uint32_t (*subclass)(void* window);
    void (*restore)(void* window);
    uint32_t (*raw_input_type)(intptr_t lparam, uint32_t* type);
    /* QueryPerformanceCounter, then QueryPerformanceFrequency. */
    void (*timestamp)(uint64_t* counter, uint64_t* frequency);
    /* rsf_fg12_cpu. */
    void (*cpu)(const rsf_game_cpu_event* event);
    /* The render callback's routing (package_state/routing.rs). fg_submitted, fg_window and
       fg_hudless are rsf_fg12_submitted, rsf_fg12_window and rsf_fg12_hudless. preview_present is
       rsf_unity_preview_present; texture_desc reads ID3D12Resource::GetDesc's width, height and
       format. preview_prepare reads Unity's D3D12 device and queue and prepares the Editor
       preview for the extents; zero when either refuses. */
    void (*fg_submitted)(uint64_t session, uint64_t source_frame);
    void (*fg_window)(const rsf_game_render_pass* pass);
    void (*fg_hudless)(void* commands, const rsf_game_render_pass* pass);
    uint32_t (*preview_present)(void* resource, const rsf_game_render_pass* pass, uint32_t common);
    void (*texture_desc)(void* resource, uint64_t* width, uint32_t* height, uint32_t* format);
    uint32_t (*preview_prepare)(uint32_t width, uint32_t height);
    /* The window guard's facts. preview_owns is rsf_unity_preview_owns, renderer_vulkan whether
       Unity's renderer is Vulkan, vulkan_owns rsf_uvk_owns_window. window_class is 0 for another
       process's window or an unreadable class, 1 for this process's UnityWndClass and 2 for
       another class in this process; the class is read only for this process's windows. */
    uint32_t (*preview_owns)(void* window);
    uint32_t (*renderer_vulkan)(void);
    uint32_t (*vulkan_owns)(void* window);
    uint32_t (*window_class)(void* window);
    /* Unity's device lifecycle, under package.cpp's mutex. bridge_load loads the renderer bridge
       with Unity's interfaces; zero, loading nothing, before UnityPluginLoad stored them.
       bridge_unload is rsf_unity_package_bridge_unload. */
    uint32_t (*bridge_load)(void);
    void (*bridge_unload)(void);
} rsf_unity_package_platform;

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

/* Render path, under package.cpp's mutex. */
/* The host render configuration; zero when output is short. */
int rsf_unity_package_plan(rsf_game_render_config* output);
/* The render callback's SR pass (begin, role RSF_GAME_RENDER_SR) on the bridge's command list,
   after the editor preview is prepared. Applies the stored gameplay screen to a copy of the pass,
   releases every view on a reconfiguring pass, replaces a view whose extents no longer fit,
   creates a missing one (Auto resolved, a failed vendor backend retried as backend 0) and
   evaluates it in the slot bound to commands once the camera is valid and no reconfiguration is
   pending. */
void rsf_unity_package_render(void* commands, const rsf_game_render_pass* pass);
/* Allocate a nonzero epoch for the lifetime of a bridge's command ring. After a successful
   list reset on the render thread, report the new recording and the prior serial proven
   complete or discarded before Execute. Serial is monotonically increasing per slot.
   No retirement is implied by the addresses of the list or fence. A checked full drain
   may retire the entire epoch once its owner stops recording. */
uint64_t rsf_unity_package_sr_ring_begin(void);
void rsf_unity_package_sr_recording(void* commands, uint64_t epoch, uint32_t slot,
    uint64_t recording, uint64_t retired_through);
void rsf_unity_package_sr_ring_retired(uint64_t epoch);
/* D3D bridge helpers that own the epoch and serials. The bridge reports a verified reset
   before its package render callback and reports release only after a checked full drain. */
void rsf_unity_package_sr_slot_reset(uint32_t bridge, void* commands, uint32_t slot);
void rsf_unity_package_sr_slots_released(uint32_t bridge);

/* Presentation, outside package.cpp's mutex except where a locked export calls it. load runs at
   plugin load, before any session: it records whether this is the Editor, reads the identity from
   [UnitySR] beside the package DLL: Runtime, resolved like every [UnitySR] path, and
   EngineVersion and ProjectId, opens the [UnitySR] Log path when one is set, and installs
   generation in the player. */
void rsf_unity_package_load(const rsf_unity_package_platform* platform);
uint32_t rsf_unity_package_editor(void);
/* A renderer bridge failure reported for the rest of the session. */
void rsf_unity_package_bridge_failed(int32_t result);

/* Session services, outside package.cpp's mutex. log writes text (ignored when null) to the
   debugger and the open log file, one line at a time. window_message reports a message dequeued
   by the hooked input window as an RSF_GAME_CPU_INPUT_EVENT (keyboard and mouse messages, and
   WM_INPUT by its raw device type, carry their kind; any other message kind 0) stamped now and
   keyed to the CPU frame in progress. input is rsfu_input's body: RSF_BACKEND_ERROR_INVALID_ARGUMENT for frame 0 or
   an unknown kind bit, nothing for no kinds, else the kinds reported now as received input,
   keyed to frame while it is the CPU frame in progress and unframed otherwise.
   preview_input hooks the Editor preview's parent once it exists while no input window is
   hooked. unload restores the input window's procedure and closes the log. */
void rsf_unity_package_log(const char* text);
int rsf_unity_package_input(uint64_t frame, uint32_t kinds);
void rsf_unity_package_preview_input(void);
void rsf_unity_package_unload(void);

/* Routing (package_state/routing.rs). route is the host render callback's body, outside
   package.cpp's mutex: a null list or pass does nothing; an end reports a non-window pass
   submitted; the window pass requests the generation provider, then presents the Editor preview
   (logging the first four refusals of the process with a texture) or hands the pass to
   generation; the final scene goes to generation as the HUD-less image; any other pass prepares
   the Editor preview and returns non-zero when the SR pass (rsf_unity_package_render) should run
   on the original pass under the mutex. accept_window is the generation window guard: in the
   Editor only the preview's window, under Vulkan only the player's surface, otherwise this
   process's UnityWndClass window or the Vulkan surface. */
uint32_t rsf_unity_package_route(void* commands, const rsf_game_render_pass* pass, uint32_t begin);
int rsf_unity_package_accept_window(void* hwnd);
/* Unity's device lifecycle, under package.cpp's mutex. device_event handles an initialize
   (non-zero) or shutdown event: a waiting device loads the bridge once Unity's interfaces exist;
   a ready one retires the session and unloads the bridge, or is retained for the process when
   retirement refuses. device_retained refuses a later plugin load. shutdown is rsfu_shutdown's
   body after the preview is hidden: 1 for a waiting device, 0 for a retained one, else the
   retirement's verdict. rsf_unity_package_initialize returns 0 for a retained device and 2 for a
   waiting one before it opens a session. */
void rsf_unity_package_device_event(uint32_t initialize);
uint32_t rsf_unity_package_device_retained(void);
uint32_t rsf_unity_package_shutdown(void);
#ifdef __cplusplus
}
#endif
