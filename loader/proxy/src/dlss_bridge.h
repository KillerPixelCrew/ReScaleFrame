/* SPDX-License-Identifier: GPL-3.0-only */
/* Private AC7 carrier interface for plugin preparation, SR, compatibility reinsertion and
   shared overlay/presentation callbacks. Install configuration/actions before observation starts.
   Except for request/request_shutdown and atomic view-size reads, mutate rendering state on its
   graphics owner thread or during serialized startup before callbacks can execute. */

#ifndef RSF_DLSS_BRIDGE_H
#define RSF_DLSS_BRIDGE_H

#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/game_renderer.h>

/* Synchronous log callback; message is borrowed terminated text for this call only. */
typedef void (*rsf_bridge_log_fn)(void* user, const char* message);

/* Prepare the plugin/fingerprint before graphics activation. The bridge copies host services
   and retains its session until native callbacks drain. Returns non-zero on accepted preparation.
   native_owned reports active native rendering, not executable recognition or game validation. */
int rsf_bridge_native_owned(void);
int rsf_bridge_prepare_game(const wchar_t* plugin_path, const char* executable_sha256,
                            rsf_bridge_log_fn log, void* log_user);


/* Start SR on the observed game device using output dimensions in pixels and quality 0..4.
   Returns non-zero when pipeline/tap/preset initialization succeeds. Early device absence can be
   retried; production auto-start records failure until enablement explicitly requests another try. */
int rsf_bridge_start(const char* streamline_directory, unsigned long output_width,
                     unsigned long output_height, unsigned long quality, rsf_bridge_log_fn log,
                     void* log_user);

/* Report stage, route and resource counters to the registered log sink. */
void rsf_bridge_report(void);

/* Arm a pipeline-output dump and bounded render-route snapshots under the supplied non-null
   terminated prefix. Readbacks run on the graphics thread and can stall it. */
void rsf_bridge_request_dump(const char* prefix);

/* Toggle a debug blit of ungraded scene reconstruction over the completed frame.
   The blit omits the game's grade/UI and requires a started pipeline. */
void rsf_bridge_toggle_display(void);

/* Toggle compatibility substitution before the game's tonemap/UI passes, requiring a
   discovered tail and live reconstruction. Native mode delegates to the enable setting.
   An armed plan does not establish image correctness. */
void rsf_bridge_toggle_reinsert(void);

/* Observer callbacks for graphics-owner frame work and actual Present completion bookkeeping. */
rsf_observer_present_fn rsf_bridge_present_hook(void);
rsf_observer_present_event_fn rsf_bridge_present_event_hook(void);

/* Atomically combine request bits for one execution at the next Present. Repeated identical
   bits coalesce; toggles execute once per consumed batch. Call from a hotkey/frontend thread
   instead of creating resources or calling the D3D11 immediate context there. */
#define RSF_BRIDGE_REQUEST_START 0x1u
#define RSF_BRIDGE_REQUEST_DISPLAY 0x2u
#define RSF_BRIDGE_REQUEST_REINSERT 0x4u
#define RSF_BRIDGE_REQUEST_EXTRACT 0x8u
void rsf_bridge_request(unsigned long requests);

/* Publish the borrowed log sink/user before the first observer callback. The sink must
   remain callable until native quiesce/stop and callback retirement complete. */
void rsf_bridge_set_log(rsf_bridge_log_fn log, void* log_user);


/* Configure bounded hexadecimal force/skip hash lists before shader creation. Lists are
 * parsed and copied; source strings are borrowed for this call only. Forced entries win.
 * Existing shaders are not reclassified by this setter. */
void rsf_bridge_name_shaders(const char* forced, const char* skipped);

/* Select UI extraction display transport: 0 no encoding, 1 sRGB, 2 gamma 2.2.
 * Other values select sRGB. The captured widget destinations are linear UNORM; display
 * conversion is applied at the final composite. */
void rsf_bridge_set_ui_encoding(int encoding);

/* Create the owned UI registry and enable observation-only classification. Install the
   creation hooks below before the observer begins; returns zero on allocation failure. */
int rsf_bridge_identify_ui(void);

/* Create an output-sized UI layer/compositor and divert classified widget quads into it.
   Requires classification and an observed device. Failure returns zero; layer creation is rolled
   back when composition initialization fails so the game UI remains in its original route. */
int rsf_bridge_extract_ui(unsigned long width, unsigned long height);

/* Object-creation callbacks consume borrowed declarations/bytecode and forget reused identities. */
rsf_observer_layout_fn rsf_bridge_layout_hook(void);
rsf_observer_shader_fn rsf_bridge_shader_hook(void);
rsf_observer_texture_fn rsf_bridge_texture_hook(void);

/* Carrier actions copied into the bridge at startup. Callbacks run from Present on the
   graphics owner thread and may be null; callers check availability. Callback code/storage must
   remain alive through bridge shutdown. Allocation, cvar patching and diagnostics belong to the
   carrier; provider/frame work belongs to the runtime modules it invokes. */
typedef struct rsf_bridge_actions {
    void (*start_backend)(void);
    int (*set_render_scale)(unsigned long percent);
    void (*trigger_dump)(void);
    void (*trigger_capture)(void);
    unsigned long (*capture_count)(void);
    unsigned long (*render_scale_percent)(void);
    /* Completed frame's instanced translucency index count, reported on the render thread.
       The carrier selects its independent allocation-scale policy. */
    void (*translucent_geometry)(unsigned long indices);
    /* Compatibility promoted-colour/depth mismatch policy: 0 drop depth, 1 keep pair,
       2 refuse promotion. Null selects drop. Read when the plan is rebuilt. */
    unsigned long (*reinsert_depth_policy)(void);
    /* Compatibility AA-gate control. Native mode manages its own view jitter. Availability
       is zero when the researched patch site was refused; null callbacks disable this control. */
    void (*set_jitter)(unsigned long open);
    unsigned long (*jitter_open)(void);
    unsigned long (*jitter_available)(void);
    unsigned long (*startup_ready)(void);
    void (*save_quality)(unsigned long quality);
    void (*save_enabled)(unsigned long enabled);
    void (*maintain_renderer)(void);
} rsf_bridge_actions;

/* Copy the actions table; a null pointer clears it. Source table storage need not be retained. */
void rsf_bridge_set_actions(const rsf_bridge_actions* actions);
/* Render-thread requested enablement. Auto-start waits for carrier patches and an observed
   device/extent; accepted enable/preset changes invoke persistence actions. */
void rsf_bridge_set_enabled(int enabled);
/* Accept quality 0..4. While running, query the backend render extent and apply the engine
   scale; on refusal restore the prior mode when possible and return zero. */
int rsf_bridge_select_quality(unsigned long quality);
/* Optional bounded research capture, armed before shader creation. Empty disables it. */
void rsf_bridge_set_briefing_capture(const char* prefix);

/* Copy SR provider directories in FSR2/FSR3/FSR4/XeSS order; null clears an entry.
   Configure before backend startup; copying a path does not validate or load its runtime. */
void rsf_bridge_set_sdk_directories(const char* fsr2, const char* fsr3, const char* fsr4, const char* xess);
/* Current bridge pipeline-started flag; call on the owner thread. It is not an atomic
   readiness snapshot and does not report effective FG or image validation. */
int rsf_bridge_running(void);
/* Request from a frontend thread. The graphics owner quiesces native producers and keeps the
   backend/device alive until queued plugin work has drained; no work runs under DllMain. */
void rsf_bridge_request_shutdown(void);

/* Read individually atomic main-view dimensions from the last qualifying/native pass.
   Both output pointers are required; zero precedes discovery. The pair is not an atomic snapshot. */
void rsf_bridge_view_size(unsigned long* width, unsigned long* height);

/* Set compatibility UI unjittering by overriding verified view-buffer twins. Startup
   configuration is retained; resources/callbacks are installed once the backend is running. */
void rsf_bridge_set_unjitter(int on);
/* Unjitter separate translucency and bypass its temporal integration. Zero selects the existing
   jittered 1:1 integration for comparison. Resolution is controlled independently by the carrier. */
void rsf_bridge_set_translucency_unjitter(int on);

#endif /* RSF_DLSS_BRIDGE_H */
