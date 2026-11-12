/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_OVERLAY_API_H
#define RSF_OVERLAY_API_H

#include <rescaleframe/runtime.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/render_frontend.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_OVERLAY_API_VERSION 1u
#define RSF_OVERLAY_API_ENTRY_POINT "rsf_get_overlay_api"

/* DLL access to the Runtime's one shared overlay for adapters that present through
   D3D11, normally the facade chain of the D3D11 presentation bridge
   (d3d11_present_api.h). It is the same overlay_host/overlay_input/overlay_renderer
   path the D3D11 carrier uses, not a second panel, renderer or runtime. The Rust panel
   (rescaleframe_overlay.dll, overlay ABI 13) is loaded from RSF_OVERLAY_DLL when that
   names a file, otherwise from beside ReScaleFrame.Runtime.dll.

   Composition. present() draws into the chain's current D3D11 backbuffer at the
   output (backbuffer) resolution, after the game's last draw and immediately before
   that Present. Call it from the bridge's before_present callback, or from the
   adapter's Present hook just before forwarding Present. On the bridge this is
   before the upload to the physical backbuffer, after every SR evaluation and after
   the HUD-less/FG input tags for that frame, so the panel is never an SR or HUD-less
   input. It is part of the real (presented) frame. Generated frames receive it
   through the provider's UI handling: extracted against the HUD-less image in the
   BACKBUFFER_HUDLESS modes, interpolated under UI_MODE_NONE. This path has no
   separate post-generation UI layer (see the frame.ui note in d3d11_present_api.h).

   Policy stays in the adapter. The adapter supplies its identity, configuration
   file and section, defaults, supported choices, window filter and callbacks as
   data. The service contains no game detection, input conventions or settings
   translation: a settings change is reported through settings_changed and the
   adapter maps it to its own renderer.

   Settings are stored in the adapter's INI section with the shared render frontend
   keys: SR, SRBackend, DLSSPreset, FG, FGBackend, FGGeneratedFrames, Latency,
   FrameLimitUs, StatusOverlay, SRQuality. Other keys in that section are kept.
   Frame-generation requests are forwarded to the shared presentation owner: the
   mode, generated count, latency mode and frame limit to rsf_native_fg_options_set,
   and a provider change to rsf_d3d11_present_request (drained, at a later Present).

   Input. Opening the panel subclasses the presenting window (WM_ mouse/keyboard,
   WM_INPUT raw mouse) and detours user32 cursor calls (ShowCursor, SetCursor,
   SetCursorPos, ClipCursor, GetCursorPos, GetRawInputData/Buffer). While open, a
   game's ShowCursor(FALSE) hide loop sees a decreasing logical count and terminates;
   the real cursor stays visible for the panel. DirectInput and XInput device reads
   are not intercepted: an adapter that polls devices must call capture() every
   frame and drop mouse/keyboard game input while it reports them captured. The
   toggle key is Insert. */

#define RSF_OVERLAY_CAPTURE_MOUSE 0x1u
#define RSF_OVERLAY_CAPTURE_KEYBOARD 0x2u

/* notify() reasons. */
/* Before ResizeBuffers or a mode change. Nothing is retained across Present, so no
   GPU object is released; the next present() lays out at the new size. */
#define RSF_OVERLAY_NOTIFY_RESIZE 1u
/* Before the chain or device the overlay drew with is released or recreated. Closes
   the panel, releases its renderer and window subclass; the next present() restarts
   on the new chain's device. Uninstalling a window procedure cannot wait for a
   message already inside it, so call where the message thread is elsewhere. */
#define RSF_OVERLAY_NOTIFY_DEVICE_RESET 2u

/* What the adapter's super-resolution path actually did for the presented frame. */
#define RSF_OVERLAY_SR_INPUT_COLOR 0x1u
#define RSF_OVERLAY_SR_INPUT_DEPTH 0x2u
#define RSF_OVERLAY_SR_INPUT_MOTION 0x4u
#define RSF_OVERLAY_SR_INPUT_EXPOSURE 0x8u
#define RSF_OVERLAY_SR_INPUT_JITTER 0x10u
typedef struct rsf_overlay_sr_report {
    uint32_t struct_size;
    /* SR backend whose output reached this frame; zero when none did. */
    uint32_t effective_backend;
    /* Quality 0..4 (native..ultra performance) of that output. */
    uint32_t effective_quality;
    uint32_t render_width, render_height, output_width, output_height;
    uint32_t frames_evaluated, frames_refused;
    int32_t last_result;
    uint32_t inputs;
    /* Nonzero when the engine owns input resolution and quality is informational. */
    uint32_t quality_locked;
    float jitter_pixels[2];
    /* Why effective differs from requested; empty when it does not. */
    char reason[96];
} rsf_overlay_sr_report;

typedef struct rsf_overlay_service_setup {
    uint32_t struct_size, abi_version;
    /* Adapter name for log lines. Never selects behaviour. Copied. */
    const char* identity_utf8;
    /* INI file and section for persisted settings. Copied. */
    const wchar_t* configuration_path;
    const wchar_t* configuration_section;
    /* Values for keys missing from the section. Must satisfy the choices below.
       struct_size/abi_version as for rsf_render_settings_get. */
    rsf_render_settings defaults;
    /* Bit n: SR backend id n the adapter can apply. Nonzero. */
    uint32_t sr_backend_choices;
    /* Bit n: SR quality n (0 native .. 4 ultra performance). Nonzero. */
    uint32_t sr_quality_choices;
    /* Bit n: FG provider id n (RSF_FG_BACKEND_*), RSF_FG_BACKEND_AUTO allowed. The
       panel offers the intersection with rsf_d3d11_present_backend_choices(). Zero
       when the adapter cannot generate frames; FG then stays Off. */
    uint32_t fg_backend_choices;
    /* Bit n: RSF_REFLEX_* latency mode n. Bit RSF_REFLEX_OFF must be set. */
    uint32_t latency_choices;
    /* Upper bound for FGGeneratedFrames, at least 1. */
    uint32_t max_generated_frames;
    /* Nonzero when the adapter applies DLSSPreset. */
    uint32_t dlss_preset_available;
    rsf_game_log_fn log;
    /* Window filter: nonzero accepts the HWND a presented chain targets. Null
       accepts every chain passed to present(). */
    int (*accept_window)(void* user, void* hwnd);
    /* Present-owner thread, inside present(). Fill the report for this frame and
       return nonzero; zero or null reports no SR output. */
    int (*sr_report)(void* user, rsf_overlay_sr_report* report);
    /* Called after a validated change was persisted, on the thread that applied it
       (present() for panel input, the caller for settings_apply). The adapter
       applies SR choices at its own drained boundary. Borrowed for the call. */
    void (*settings_changed)(void* user, const rsf_render_settings* applied);
    void* user;
} rsf_overlay_service_setup;

typedef struct rsf_overlay_capture {
    uint32_t struct_size;
    /* The panel is loaded and attached to a window. */
    uint32_t started;
    uint32_t visible;
    /* RSF_OVERLAY_CAPTURE_* the game must not act on this frame. */
    uint32_t captured;
} rsf_overlay_capture;

/* Results are rsf_result values.
   - configure: once, cold, outside DllMain, before the first present(). Strings and
     defaults are copied; callbacks and user must stay valid for process lifetime.
     Saved FG options are forwarded to the presentation owner. A second call: BUSY.
   - present: Present-owner thread, see Composition. Starts the overlay on the first
     accepted chain. drawn (optional) is set to 1 when the panel, HUD or startup hint
     was drawn. NOT_READY: not configured, window refused, or the panel could not
     start (a failed start is final until NOTIFY_DEVICE_RESET).
   - notify: Present-owner thread, before the chain/device change; see reasons.
   - visible/capture: any thread, lock-free, cheap enough for every frame.
   - set_visible/toggle: any thread. NOT_READY before the panel started; BUSY when a
     change landed within the 250 ms debounce and the state did not change.
   - status: any thread. Requested values are the persisted settings; effective
     values are the adapter's last sr_report and the presentation owner's provider
     status sampled at the last present().
   - settings_get/settings_apply: any thread except from settings_changed. apply
     validates against the setup choices, persists, forwards FG and then calls
     settings_changed. Refused requests change nothing. */
typedef struct rsf_overlay_api {
    uint32_t struct_size;
    uint32_t abi_version;
    int32_t (*configure)(const rsf_overlay_service_setup* setup);
    int32_t (*present)(void* swapchain, uint32_t* drawn);
    int32_t (*notify)(void* swapchain, uint32_t reason);
    uint32_t (*visible)(void);
    int32_t (*set_visible)(uint32_t visible);
    int32_t (*toggle)(void);
    int32_t (*capture)(rsf_overlay_capture* capture);
    int32_t (*status)(rsf_render_status* status);
    int32_t (*settings_get)(rsf_render_settings* settings);
    int32_t (*settings_apply)(const rsf_render_settings* settings);
} rsf_overlay_api;

/* Same contract as rsf_get_d3d11_present_api: initialize struct_size to writable
   capacity and abi_version to RSF_OVERLAY_API_VERSION. Null or capacity below
   sizeof(rsf_overlay_api): INVALID_ARGUMENT, without reading abi_version. Otherwise a
   version mismatch: ABI_MISMATCH. Failure writes nothing. On OK the table is filled
   and struct_size is the runtime's sizeof(table). Querying loads nothing. */
RSF_RUNTIME_API rsf_result rsf_get_overlay_api(rsf_overlay_api* api);
typedef rsf_result (*rsf_get_overlay_api_fn)(rsf_overlay_api* api);

#ifdef __cplusplus
}
#endif
#endif
