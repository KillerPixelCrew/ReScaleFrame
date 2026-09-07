/* SPDX-License-Identifier: GPL-3.0-only */
/* The research proxy's DLSS path, kept behind four functions so the proxy itself stays readable.

   This is not a public interface. It exists inside the loader because the loader is what gets into
   the game today, and every part of it belongs elsewhere: see the comment at the top of
   dlss_bridge.c and the ownership rules in AGENTS.md. */

#ifndef RSF_DLSS_BRIDGE_H
#define RSF_DLSS_BRIDGE_H

#include <rescaleframe/d3d11_observer.h>

typedef void (*rsf_bridge_log_fn)(void* user, const char* message);

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

/* Ask for the next evaluated frame's output to be written out. */
void rsf_bridge_request_dump(const char* prefix);

/* Show the reconstruction over the game's own frame, or stop showing it.

   The counters say it ran and a dumped frame says the geometry is right, but the faults that matter
   most in an upscaler are temporal: ghosting, a smear behind a moving object, a motion vector whose
   sign is inverted. None of those exist in a still image. This is how they get looked at, and it is
   a debug view: the image is ungraded scene colour with no interface on it. */
void rsf_bridge_toggle_display(void);

/* Put the reconstruction back into the game's own frame, or stop.

   This is the real path rather than the debug view above: the scene is reconstructed before the
   game's tonemap, the game grades it and composites its own interface onto it at output resolution,
   and its final upscale becomes a copy. It needs the frame's tail to have been identified, which
   takes the first few frames after the backend starts.

   Off until asked for, because it changes what the game draws. */
void rsf_bridge_toggle_reinsert(void);

/* The Present callback the observer should be given, so the display above has a place to draw. */
rsf_observer_present_fn rsf_bridge_present_hook(void);

/* Where this speaks, set at install rather than at F8.

   Starting the backend used to be the first thing that gave the bridge a log sink, which was fine
   while everything it did happened after F8. The overlay does not: it comes up as soon as the game
   has a device so that it can be opened to see that nothing is running. Without this it would be
   silent, and would not come up at all. */
void rsf_bridge_set_log(rsf_bridge_log_fn log, void* log_user);

/* What the overlay can ask for that this file does not own.

   Starting the backend, the render scale and frame capture all live in the carrier, because they
   need its environment parsing, its console-variable addresses and its RenderDoc handle. The
   overlay runs from the present hook in here, so the panel's requests arrive here and are handed
   back out through these. Any member may be null, and a request for a null one is refused out loud
   rather than silently dropped.

   Registered at attach, next to the log sink and for the same reason. */
typedef struct rsf_bridge_actions {
    void (*start_backend)(void);
    void (*set_render_scale)(unsigned long percent);
    void (*trigger_dump)(void);
    void (*trigger_capture)(void);
    unsigned long (*capture_count)(void);
    unsigned long (*render_scale_percent)(void);
    /* How many indices this frame's separate translucency draws carried, reported once per frame.
       The carrier owns what to do about it: the resolution that layer renders at is a patched
       immediate over there, and the thresholds are settings, so this reports and does not decide.
       Called from the present hook, on the render thread, like the rest of these. */
    void (*translucent_geometry)(unsigned long indices);
} rsf_bridge_actions;

void rsf_bridge_set_actions(const rsf_bridge_actions* actions);

int rsf_bridge_running(void);

#endif /* RSF_DLSS_BRIDGE_H */
