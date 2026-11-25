/* SPDX-License-Identifier: MIT */
/* The research proxy's native SR path, kept behind a few functions so the proxy itself stays readable.

   This is not a public interface. It exists inside the loader because the loader is what gets into
   the game today, and every part of it belongs elsewhere: see the comment at the top of
   dlss_bridge.c and the ownership rules in AGENTS.md. */

#ifndef RSF_DLSS_BRIDGE_H
#define RSF_DLSS_BRIDGE_H

#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/sharpening_settings.h>

typedef void (*rsf_bridge_log_fn)(void* user, const char* message);

/* Native game-controller lifecycle. Preparation belongs before graphics activation. */
int rsf_bridge_prepare_game(const wchar_t* plugin_path, const rsf_game_probe* probe,
                            rsf_bridge_log_fn log, void* log_user);

/* Unreal's velocity encoding, from Common.ush: In * (0.499 * 0.5) + 32767/65535. Written here as
   the decode a backend needs, which is the reciprocal of that scale and the same bias. The sentinel
   is far outside any real screen space motion and still exact in half precision. */
#define RSF_UNREAL_MOTION_SCALE (1.0f / (0.499f * 0.5f))
#define RSF_UNREAL_MOTION_BIAS (32767.0f / 65535.0f)
#define RSF_MOTION_SENTINEL (-1000.0f)

/* Acquire the game's device, start the backend, and begin watching frames.

   Returns non-zero when everything came up. Returning zero is a normal outcome early in startup,
   when the game has not created a device yet, so a caller may try again rather than give up. */
int rsf_bridge_start(const char* streamline_directory, unsigned long output_width,
                     unsigned long output_height, unsigned long quality, rsf_bridge_log_fn log,
                     void* log_user);

/* Write the counters to the log. Worth doing on a key press: a run that produced no image should
   be able to say which step it stopped at, and every counter here has been the answer at least
   once. */
void rsf_bridge_report(void);

/* Ask for the next evaluated frame's output and the native route to be written out. */
void rsf_bridge_request_dump(const char* prefix);

/* The Present callbacks the observer should be given. */
rsf_observer_present_fn rsf_bridge_present_hook(void);
rsf_observer_present_event_fn rsf_bridge_present_event_hook(void);
/* Last presenting extent reported by the observer. Width zero means no presenting size yet. */
int rsf_bridge_present_extent(uint32_t* width, uint32_t* height);

/* Streamline identity of the game this proxy is built for (Ace Combat 7, Unreal Engine 4.18.3).
   One value for the SR pipeline and the frame generation bridge, so the two never register
   differently. ENGINE_TYPE is sl::EngineType::eUnreal. */
#define RSF_AC7_ENGINE_TYPE 1u
#define RSF_AC7_ENGINE_VERSION "4.18.3"
#define RSF_AC7_PROJECT_ID "a3ed1f08-3542-4698-b85c-e1a9908e861a"

/* Where this speaks, set at install rather than at F8.

   Starting the backend used to be the first thing that gave the bridge a log sink, which was fine
   while everything it did happened after F8. The overlay does not: it comes up as soon as the game
   has a device so that it can be opened to see that nothing is running. Without this it would be
   silent, and would not come up at all. */
void rsf_bridge_set_log(rsf_bridge_log_fn log, void* log_user);

/* Resource creation hooks for the observer: input layouts feed the motion capture and the velocity
   pass, shaders the motion capture. */
rsf_observer_layout_fn rsf_bridge_layout_hook(void);
rsf_observer_shader_fn rsf_bridge_shader_hook(void);

/* What the overlay can ask for that this file does not own.

   Starting the backend and frame capture live in the carrier, because they
   need its environment parsing, its console-variable addresses and its RenderDoc handle. The
   overlay runs from the present hook in here, so the panel's requests arrive here and are handed
   back out through these. Any member may be null, and a request for a null one is refused out loud
   rather than silently dropped.

   Registered at attach, next to the log sink and for the same reason. */
struct rsf_overlay_host_option;
typedef struct rsf_bridge_actions {
    void (*start_backend)(void);
    void (*start_backend_at_extent)(uint32_t width, uint32_t height);
    void (*trigger_dump)(void);
    void (*trigger_capture)(void);
    unsigned long (*capture_count)(void);
    /* The carrier's patched temporal jitter gate. The native renderer supplies its own jitter, so
       the bridge shuts the gate when SR starts; the panel and F4 can override. `jitter_available`
       is zero when the patch could not find its site, so the panel refuses instead of offering a
       switch that does nothing. Null means this build has no jitter control. */
    void (*set_jitter)(unsigned long open);
    unsigned long (*jitter_available)(void);
    unsigned long (*startup_ready)(void);
    void (*save_quality)(unsigned long quality);
    void (*save_dlss_preset)(unsigned long preset);
    void (*save_sharpening)(const rsf_sharpening_options*);
    void (*save_enabled)(unsigned long enabled);
    uint32_t (*read_host_options)(struct rsf_overlay_host_option* options, uint32_t capacity);
    int (*save_host_option)(uint32_t index, int32_t value);
    int (*save_frame_limit)(uint32_t interval_or_policy);
} rsf_bridge_actions;

void rsf_bridge_set_actions(const rsf_bridge_actions* actions);
/* Requested session state. Startup waits for patches/device/tail discovery on the render thread. */
void rsf_bridge_set_enabled(int enabled);
int rsf_bridge_select_dlss_preset(unsigned long preset);
int rsf_bridge_select_sharpening(const rsf_sharpening_options*);
int rsf_bridge_select_quality(unsigned long quality);
void rsf_bridge_set_sdk_directories(const char* fsr2, const char* fsr3, const char* fsr4, const char* xess);
int rsf_bridge_running(void);
/* Tells the bridge whether the motion capture is configured, so the constant watch it needs stays armed. */
void rsf_bridge_set_motion_capture(int on);

#endif /* RSF_DLSS_BRIDGE_H */
