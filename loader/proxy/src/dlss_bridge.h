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

/* Start naming the game's pipeline objects and classifying the draws made from them.

   This changes no pixel. It answers which draws on a screen are the interface, by which producer,
   and how many looked like it and matched no rule, which is the number that decides whether
   diverting them is safe to attempt. Returns zero if the registry could not be created, in which
   case nothing is classified and the run says so.

   There is deliberately no expected size here. The first run asked for textures shaped like a
   converter target at 1920x1080 and matched over a hundred and eighty of them, and also found Slate
   drawing into a 1920x3304 target that no such list would have contained. A converter target is
   confirmed by watching Slate write into it instead.

   Call before installing the observer: the three hooks below have to be in its options, because
   what an object is can only be learned as the game creates it. */
/* Name shaders by hash, in either direction, from a settings file.
 *
 * The escape hatch for a run where the rules are wrong about one draw and a rebuild is too slow.
 * Both are hex lists, separated by anything; null or empty means none. Forced wins over every rule,
 * skipped loses to none.
 *
 * The hashes to name come from the `ui draw:` trace lines, which print them for exactly this
 * reason: a shader's pointer means nothing once the process exits, and its hash is the only stable
 * name it has. */
void rsf_bridge_name_shaders(const char* forced, const char* skipped);

/* Which transfer function the composite applies to the layer: 0 none, 1 sRGB, 2 gamma 2.2.
 *
 * The layer holds what AC7's interface target held, which the trace shows is linear in a plain
 * UNORM view, while the back buffer holds colour the game has already transformed for display.
 * Compositing one onto the other untransformed is what made the extracted interface arrive dark.
 *
 * Which curve is correct is not something the graphics API records, and the two differ visibly in
 * the shadows, so this exists to tell them apart in one run rather than by argument. */
void rsf_bridge_set_ui_encoding(int encoding);

int rsf_bridge_identify_ui(void);

/* Start extracting the interface: divert the draws the classifier names into a layer at `width` by
   `height`, and composite that layer back over the finished frame at present.

   This is the first thing in the project that changes the picture on purpose. What it is for is the
   only route that can sharpen an interface AC7 rasterizes at a fixed size and then draws into the
   scene as geometry, and the composite is the same premultiplied arithmetic every frame generation
   SDK will later ask for, so the picture we make and the picture a vendor makes cannot drift.

   Needs classification running. Returns zero and says why if the layer or the compositor could not
   be built; both are refused together, because a layer with no compositor would take the interface
   out of the frame and never put it back. */
int rsf_bridge_extract_ui(unsigned long width, unsigned long height);

rsf_observer_layout_fn rsf_bridge_layout_hook(void);
rsf_observer_shader_fn rsf_bridge_shader_hook(void);
rsf_observer_texture_fn rsf_bridge_texture_hook(void);

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
    /* What reinsertion should do when a promoted target meets the game's render resolution depth:
       0 drop the depth, 1 forward both, 2 leave the target alone. Read when the plan is built, so a
       change takes effect on the next F6. Null means drop, which is the only one of the three that
       keeps the pixels. */
    unsigned long (*reinsert_depth_policy)(void);
    /* The engine's temporal jitter, which on this game is ours: AC7 runs no temporal AA, so a
       patched gate is what makes the projection move at all. Opening it where nothing resolves the
       offset shows as a shimmer, and the front end is where that shows, because it holds still.

       Luma's Unreal path never has the problem because it never manufactures the jitter: it runs
       only where the engine already ran temporal AA and treats a frame without it as a camera cut.
       We cannot copy that, but we can copy the discipline, which is what `set_jitter` is for. The
       bridge opens the gate when it starts resolving frames and the panel and F4 can override.

       `jitter_open` and `jitter_available` report; the second is zero when the patch could not
       find its site, so the panel refuses instead of offering a switch that does nothing. Null
       means this build has no jitter control and the panel shows it greyed. */
    void (*set_jitter)(unsigned long open);
    unsigned long (*jitter_open)(void);
    unsigned long (*jitter_available)(void);
} rsf_bridge_actions;

void rsf_bridge_set_actions(const rsf_bridge_actions* actions);

int rsf_bridge_running(void);

#endif /* RSF_DLSS_BRIDGE_H */
