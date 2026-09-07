/* SPDX-License-Identifier: GPL-3.0-only */
/* Wire the pieces together inside Ace Combat 7, and be honest about where this lives.

   Every part of this belongs somewhere else eventually. Finding the game's device is the loader's
   job, reading its view buffer is the plugin's, and driving a backend is the orchestrator's. They
   are here because the research proxy is what can be loaded into the game today, and because
   proving the path end to end is worth more right now than proving it in the right file. The
   ownership split in AGENTS.md says where each piece goes, and moving them is its own change.

   What this does, once per frame, on the game's render thread:

     the frame tap recognises a pass binding scene colour, depth, velocity and a 1x1 exposure
       -> copy the view uniform buffer bound around it
       -> read the camera out of it, refusing anything that is not the main view
       -> hand the pipeline the textures and the camera, which decodes the motion and evaluates

   The reason this is a bridge and not a feature is the last step of the sentence. Evaluating
   successfully is not the same as producing a correct image, and nothing here has produced one. */

#include <windows.h>

#include <rescaleframe/ac7_view.h>
#include <rescaleframe/constant_buffer_read.h>
#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/frame_tap.h>
#include <rescaleframe/present_blit.h>
#include <rescaleframe/resource_ref.h>

#include <stdio.h>
#include <string.h>

#include "dlss_bridge.h"

/* Unreal's velocity encoding, from Common.ush: In * (0.499 * 0.5) + 32767/65535. Written here as
   the decode a backend needs, which is the reciprocal of that scale and the same bias. The sentinel
   is far outside any real screen space motion and still exact in half precision. */
#define RSF_UNREAL_MOTION_SCALE (1.0f / (0.499f * 0.5f))
#define RSF_UNREAL_MOTION_BIAS (32767.0f / 65535.0f)
#define RSF_MOTION_SENTINEL (-1000.0f)

/* How long the frame's tail is looked at, in presents.

   Bounded on purpose, and not only because this runs on the render thread. Watching the back
   buffer means holding a reference on it, and a held back buffer reference makes `ResizeBuffers`
   fail, which is a mode change or an alt-tab breaking. A few frames answers the question; keeping
   the watch for the process's life would trade an answer for a fault that appears much later and
   looks like something else entirely. The composite is an engine pooled target and carries no such
   hazard, so it is watched for longer. */
#define RSF_TAIL_ARM_FRAMES 6ul
#define RSF_TAIL_STOP_FRAMES 32ul
/* Draw budgets handed to the tap. The frame ends in one draw into the back buffer, so a handful
   spans several frames. The composite takes the whole interface on top of the scene, so it takes
   more, and the ordinal in each report says which draw of the pass it was. */
#define RSF_TAIL_BACK_BUFFER_DRAWS 8u
#define RSF_TAIL_COMPOSITE_DRAWS 64u

static struct {
    int started;
    void* device;
    void* context;
    rsf_bridge_log_fn log;
    void* log_user;

    /* Counters, so a run that produced nothing can say which step it stopped at rather than just
       failing quietly. Every one of these has been the answer at some point in this project. */
    unsigned long passes;
    unsigned long view_read_failures;
    unsigned long not_main_view;
    unsigned long no_jitter;
    unsigned long evaluated;
    unsigned long refused;
    long last_result;

    /* What the last report said, so a report on a timer stays quiet while nothing moves. */
    unsigned long reported_calls;
    unsigned long reported_passes;
    unsigned long reported_evaluated;

    /* Showing the result on screen. The counters and a dumped frame cannot answer the questions
       that matter most about an upscaler, because ghosting and a smear behind a moving object are
       temporal and a still image has no time in it. */
    rsf_present_blit* blit;
    int show;
    unsigned long frames_shown;

    /* Which pass within the current frame, and how many frames have been described.
       The set is recognised more than once per frame and this integration takes the first, which
       is how a reconstruction ended up with no sky in it: the sky is composited later than the
       pass being tapped. Describing each qualifying pass of a few frames says how many there are
       and what colour each carries, which is what choosing between them needs. */
    unsigned long pass_in_frame;
    unsigned long frames_described;

    /* The frame's inputs, held from the pass that identifies them until Present.

       The evaluate cannot happen where the set is recognised. That pass is the lighting, and a
       capture replay puts twenty seven draws after it that add the sky, the clouds and the
       translucency to the very colour target it binds, which is why the reconstruction came out
       with a black sky. Those later passes do not bind velocity and depth, so they never qualify
       and there is no later set to prefer.

       What the replay also shows is that none of the three targets is written again once the
       colour is finished: the post chain only reads them. So the contents at Present are the
       finished frame, and Present is where this evaluates. */
    void* held_color;
    void* held_depth;
    void* held_motion;
    void* held_exposure;
    rsf_camera_frame held_camera;
    unsigned long held_width;
    unsigned long held_height;
    int have_held;

    /* Learning where the reconstruction goes back in.

       The result is drawn over the finished frame today, which is why it is ungraded and has no
       interface. Putting it back properly means replacing the scene before the game composites its
       interface onto it, and at a reduced render scale that composite runs at render resolution:
       ac7-frame-capture.md has the frame ending in one draw that reads a single 1024x576 composite
       and writes the 2048x1152 back buffer.

       Which draws write that composite, and which of them reads scene colour, is the one fact that
       places the intervention and it is in no capture here. The exported action list records render
       target bindings and not shader resource bindings, which that document states as a limitation
       twice. So it is asked of the running game: watch the back buffer, and whatever its draw
       reads is the composite; watch the composite, and its draws say which one is the tonemap.

       Both are retained for as long as they are watched, because the tap compares by address and
       does not hold a reference of its own. */
    void* back_buffer;
    void* composite;
    /* Kept separately from the pointer, which is dropped when the watch ends. What was learned
       outlives the reference that was needed to learn it. */
    int composite_found;
    unsigned long tail_frames;
    unsigned long tail_draws;
} bridge;

static void release_held(void)
{
    rsf_resource_release(bridge.held_color);
    rsf_resource_release(bridge.held_depth);
    rsf_resource_release(bridge.held_motion);
    rsf_resource_release(bridge.held_exposure);
    bridge.held_color = NULL;
    bridge.held_depth = NULL;
    bridge.held_motion = NULL;
    bridge.held_exposure = NULL;
    bridge.have_held = 0;
}

static void say(const char* format, ...)
{
    char message[512];
    va_list arguments;
    if (!bridge.log) {
        return;
    }
    va_start(arguments, format);
    vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    bridge.log(bridge.log_user, message);
}

/* Turn what the view buffer says into what a backend is told. Only the fields the reader actually
   established: the pipeline fills the motion ones because it is the code that decodes them, and
   inventing the rest here would be exactly the kind of plausible wrong value this project keeps
   catching. */
static void fill_camera(const rsf_ac7_view* view, rsf_camera_frame* camera)
{
    memset(camera, 0, sizeof(*camera));
    camera->struct_size = sizeof(*camera);
    camera->abi_version = RSF_FRAME_ASSEMBLY_ABI_VERSION;

    /* The un-jittered projection, not the one the buffer holds. 4.18 applies the offset to the
       projection itself and keeps no copy without it, and a backend given the jittered matrix
       reconstructs for a camera that was never rendered. */
    memcpy(camera->view_to_clip, view->view_to_clip_no_jitter, sizeof(camera->view_to_clip));
    memcpy(camera->clip_to_view, view->clip_to_view_no_jitter, sizeof(camera->clip_to_view));
    memcpy(camera->clip_to_prev_clip, view->clip_to_prev_clip, sizeof(camera->clip_to_prev_clip));
    memcpy(camera->prev_clip_to_clip, view->prev_clip_to_clip, sizeof(camera->prev_clip_to_clip));

    memcpy(camera->camera_position, view->camera_position, sizeof(camera->camera_position));
    memcpy(camera->camera_forward, view->camera_forward, sizeof(camera->camera_forward));
    memcpy(camera->camera_up, view->camera_up, sizeof(camera->camera_up));
    memcpy(camera->camera_right, view->camera_right, sizeof(camera->camera_right));

    camera->near_plane = view->near_plane;
    /* Zero means infinite, which is what a reversed Z projection has and what the assembly turns
       into a number a backend can use. */
    camera->far_plane = 0.0f;
    camera->vertical_fov = view->vertical_fov;
    camera->aspect_ratio = view->aspect_ratio;

    camera->jitter_pixels[0] = view->jitter_pixels[0];
    camera->jitter_pixels[1] = view->jitter_pixels[1];
    camera->has_jitter = view->has_jitter;

    /* Reversed Z, established from the projection's own form and agreed with independently by
       InvDeviceZToWorldZTransform. Unreal writes object motion only, which is measured rather than
       assumed: 83.6% of a camera-panning frame is exactly zero. */
    camera->depth_inverted = 1u;
    camera->camera_motion_included = 0u;
}

/* Called by the frame tap, on the game's render thread, inside the pass that binds the inputs. This
   is the one moment those textures hold this frame's contents, so the work happens here rather than
   being remembered for later. */
static void on_pass(void* user, const rsf_frame_tap_pass* pass)
{
    unsigned char view_bytes[RSF_AC7_VIEW_BUFFER_BYTES];
    rsf_ac7_view view;
    rsf_camera_frame camera;
    rsf_dlss_pipeline_frame frame;
    rsf_dlss_pipeline_result result;

    (void)user;
    if (!bridge.started || !pass || !pass->view_constants) {
        return;
    }
    ++bridge.passes;

    if (rsf_read_constant_buffer(bridge.device, pass->context, pass->view_constants, view_bytes,
                                 sizeof(view_bytes)) != RSF_CONSTANT_BUFFER_OK) {
        ++bridge.view_read_failures;
        return;
    }

    memset(&view, 0, sizeof(view));
    view.struct_size = sizeof(view);
    if (rsf_ac7_view_read(view_bytes, (uint32_t)sizeof(view_bytes), RSF_AC7_VIEW_ABI_VERSION,
                          &view) != RSF_AC7_VIEW_OK) {
        ++bridge.view_read_failures;
        return;
    }

    /* The engine renders several views into the same target. Handing a backend the camera of one
       the player is not looking through produces a plausible, wrong image, which is worse than
       producing none. */
    if (!view.is_main_view) {
        ++bridge.not_main_view;
        return;
    }
    if (!view.has_jitter) {
        /* Expected until the anti-aliasing gate is patched, and worth counting separately: a run
           that reaches here and stops has found everything except the one thing RSF_ENABLE_JITTER
           turns on. */
        ++bridge.no_jitter;
        return;
    }

    ++bridge.pass_in_frame;
    if (bridge.frames_described < 4) {
        /* The colour pointer as well as its format. The frame binds this set more than once and
           the passes differ in what their colour holds, so which one is being taken is the
           question: a capture replay puts the sky twenty seven draws after the lighting, and a
           colour taken before those has no sky in it. */
        say("  pass %lu of this frame: colour %p format %lu at %ux%u, %s exposure",
            bridge.pass_in_frame, pass->scene_color, (unsigned long)pass->scene_color_format,
            pass->render_width, pass->render_height, pass->exposure ? "with" : "no");
    }

    fill_camera(&view, &camera);

    /* Held rather than evaluated. The camera is a plain structure and is copied; the textures are
       borrowed for this callback only, so keeping them past it means taking a reference. One set
       per frame: if a second pass somehow qualifies, the first is dropped rather than leaked. */
    if (bridge.have_held) {
        release_held();
    }
    bridge.held_color = pass->scene_color;
    bridge.held_depth = pass->depth;
    bridge.held_motion = pass->motion;
    bridge.held_exposure = pass->exposure;
    rsf_resource_retain(bridge.held_color);
    rsf_resource_retain(bridge.held_depth);
    rsf_resource_retain(bridge.held_motion);
    rsf_resource_retain(bridge.held_exposure);
    bridge.held_camera = camera;
    bridge.held_width = pass->render_width;
    bridge.held_height = pass->render_height;
    bridge.have_held = 1;
    (void)frame;
    (void)result;
}

/* One reported draw's pixel shader inputs, one line each.

   Slot numbers are kept rather than renumbered, so a gap says a slot held something that is not a
   2D texture. The scene colour is marked where it appears, and that mark is the answer being looked
   for: the draw into the composite that reads it is the tonemap, and the tonemap is where the
   reconstructed scene has to go in. */
static void describe_inputs(const rsf_frame_tap_target_draw* draw)
{
    uint32_t index;
    for (index = 0; index < draw->input_count; ++index) {
        const rsf_frame_tap_input* input = &draw->inputs[index];
        say("    slot %lu: %p %lux%lu format %lu%s", (unsigned long)input->slot, input->texture,
            (unsigned long)input->width, (unsigned long)input->height,
            (unsigned long)input->format,
            (input->texture && input->texture == bridge.held_color) ? "  <- scene colour" : "");
    }
}

/* Called by the frame tap, on the render thread, after a draw into a target this asked about. */
static void on_target_draw(void* user, const rsf_frame_tap_target_draw* draw)
{
    (void)user;
    if (!draw) {
        return;
    }
    ++bridge.tail_draws;

    say("%s draw %lu: target %p %lux%lu format %lu, viewport %lux%lu, %s %lu, %lu inputs",
        draw->watch_index == 0 ? "back buffer" : "composite", (unsigned long)draw->draw_index,
        draw->render_target, (unsigned long)draw->target_width,
        (unsigned long)draw->target_height, (unsigned long)draw->target_format,
        (unsigned long)draw->viewport_width, (unsigned long)draw->viewport_height,
        draw->indexed ? "indices" : "vertices", (unsigned long)draw->element_count,
        (unsigned long)draw->input_count);
    describe_inputs(draw);

    /* What the back buffer draw reads is the composite. At a reduced render scale it reads exactly
       one resource, so anything else is a different tail than the one the capture describes, and
       taking the first input regardless would name the wrong texture and describe the wrong pass.
       Refusing here costs one game run and is the difference between an answer and a guess. */
    if (draw->watch_index != 0 || bridge.composite) {
        return;
    }
    if (draw->input_count != 1 || !draw->inputs[0].texture) {
        return;
    }
    /* Our own debug blit also draws over the back buffer reading exactly one texture, from inside
       the Present hook, and nothing about its shape distinguishes it from the game's last draw.
       Taking it would point the reinsertion at the reconstruction's own output. */
    if (draw->inputs[0].texture == rsf_dlss_pipeline_output_texture()) {
        return;
    }
    bridge.composite = draw->inputs[0].texture;
    bridge.composite_found = 1;
    rsf_resource_retain(bridge.composite);
    rsf_frame_tap_watch_target(1, bridge.composite, RSF_TAIL_COMPOSITE_DRAWS);
    say("  that single input is the composite the scene has to be replaced in. Watching it: the "
        "draw into it that reads scene colour is where the reconstruction goes back");
}

/* Ask the frame about its own tail, for a bounded number of presents. */
static void watch_tail(void* swapchain)
{
    void* buffer;

    ++bridge.tail_frames;
    if (bridge.tail_frames > RSF_TAIL_STOP_FRAMES) {
        return;
    }
    if (bridge.tail_frames == RSF_TAIL_STOP_FRAMES) {
        rsf_frame_tap_watch_target(1, NULL, 0);
        rsf_resource_release(bridge.composite);
        bridge.composite = NULL;
        say("frame tail: done looking, %lu draws described", bridge.tail_draws);
        return;
    }
    if (bridge.tail_frames >= RSF_TAIL_ARM_FRAMES) {
        if (bridge.back_buffer) {
            rsf_frame_tap_watch_target(0, NULL, 0);
            rsf_resource_release(bridge.back_buffer);
            bridge.back_buffer = NULL;
        }
        return;
    }

    /* Re-read rather than kept. A flip model swap chain hands out a different texture per frame,
       and a watch left on the previous one would match nothing while looking exactly like a tail
       that has no draws in it. */
    buffer = rsf_swapchain_back_buffer(swapchain);
    if (!buffer) {
        return;
    }
    if (buffer == bridge.back_buffer) {
        rsf_resource_release(buffer);
        return;
    }
    rsf_resource_release(bridge.back_buffer);
    bridge.back_buffer = buffer;
    rsf_frame_tap_watch_target(0, buffer, RSF_TAIL_BACK_BUFFER_DRAWS);
}

/* Run the frame that was held, now that the game has finished drawing it. */
static void evaluate_held(void* context)
{
    rsf_dlss_pipeline_frame frame;
    rsf_dlss_pipeline_result result;

    if (!bridge.have_held) {
        return;
    }

    memset(&frame, 0, sizeof(frame));
    frame.struct_size = sizeof(frame);
    frame.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
    frame.scene_color = bridge.held_color;
    frame.depth = bridge.held_depth;
    frame.game_motion = bridge.held_motion;
    frame.exposure = bridge.held_exposure;
    frame.render_width = (uint32_t)bridge.held_width;
    frame.render_height = (uint32_t)bridge.held_height;
    frame.camera = &bridge.held_camera;

    result = rsf_dlss_pipeline_on_frame(context, &frame);
    bridge.last_result = (long)result;
    if (result == RSF_DLSS_PIPELINE_OK) {
        ++bridge.evaluated;
    } else {
        ++bridge.refused;
    }
    release_held();
}

/* Called before the game's own Present, from the observer.

   Drawing the reconstruction over the finished frame replaces a graded image that has an interface
   on it with an ungraded one that does not, so it looks wrong in brightness and has no HUD even
   when the reconstruction is perfect. That is the price of being able to see motion at all, and it
   is a debug view rather than a step towards how this should work. */
static void on_present(void* user, void* swapchain)
{
    void* output;

    (void)user;
    if (bridge.frames_described < 4 && bridge.pass_in_frame > 0) {
        say("frame ended after %lu qualifying passes", bridge.pass_in_frame);
        ++bridge.frames_described;
    }
    bridge.pass_in_frame = 0;

    /* The frame is finished here, which is the whole reason the evaluate waits for it. */
    if (bridge.started) {
        watch_tail(swapchain);
        evaluate_held(bridge.context);
    }

    if (!bridge.started || !bridge.show || !bridge.blit) {
        return;
    }
    output = rsf_dlss_pipeline_output_texture();
    if (!output || bridge.evaluated == 0) {
        return;
    }
    if (rsf_present_blit_draw(bridge.blit, bridge.context, swapchain, output, 1u) ==
        RSF_PRESENT_BLIT_OK) {
        ++bridge.frames_shown;
    }
}

void rsf_bridge_toggle_display(void)
{
    if (!bridge.started) {
        say("dlss bridge: nothing to show, the backend is not running");
        return;
    }
    if (!bridge.blit) {
        rsf_present_blit_setup setup;
        memset(&setup, 0, sizeof(setup));
        setup.struct_size = sizeof(setup);
        setup.abi_version = RSF_PRESENT_BLIT_ABI_VERSION;
        setup.log = bridge.log;
        setup.log_user = bridge.log_user;
        if (rsf_present_blit_create(bridge.device, &setup, &bridge.blit) != RSF_PRESENT_BLIT_OK) {
            say("dlss bridge: cannot show the result, the blit would not build");
            return;
        }
    }
    bridge.show = !bridge.show;
    say("dlss bridge: showing the reconstruction is now %s. It is scene colour from partway "
        "through the frame, so it is ungraded and has no interface on it",
        bridge.show ? "on" : "off");
}

rsf_observer_present_fn rsf_bridge_present_hook(void)
{
    return on_present;
}

int rsf_bridge_start(const char* streamline_directory, unsigned long output_width,
                     unsigned long output_height, unsigned long quality, rsf_bridge_log_fn log,
                     void* log_user)
{
    rsf_dlss_pipeline_setup setup;
    rsf_frame_tap_options tap;
    rsf_dlss_pipeline_result started;
    rsf_frame_tap_result tapped;

    bridge.log = log;
    bridge.log_user = log_user;
    if (bridge.started) {
        return 1;
    }
    if (!streamline_directory || !streamline_directory[0]) {
        say("dlss bridge: RSF_STREAMLINE_BIN is not set, nothing to load");
        return 0;
    }

    /* The game's own device, not one of ours. A second device could not share resources with the
       game's, which is the entire point. */
    if (rsf_observer_acquire_device(&bridge.device, &bridge.context) != RSF_OBSERVER_OK) {
        say("dlss bridge: no device yet, the game has not created one");
        return 0;
    }

    memset(&setup, 0, sizeof(setup));
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
    setup.streamline_directory_utf8 = streamline_directory;
    setup.output_width = (uint32_t)output_width;
    setup.output_height = (uint32_t)output_height;
    setup.quality = (rsf_dlss_quality)quality;
    setup.motion.struct_size = sizeof(setup.motion);
    setup.motion.scale_x = RSF_UNREAL_MOTION_SCALE;
    setup.motion.scale_y = RSF_UNREAL_MOTION_SCALE;
    setup.motion.bias_x = RSF_UNREAL_MOTION_BIAS;
    setup.motion.bias_y = RSF_UNREAL_MOTION_BIAS;
    /* Left at one: decoded Unreal motion is already in the range a backend wants. Which sign and
       which axis direction it actually needs is unverified, and this is where a flip goes when a
       rendered result shows one is needed. */
    setup.motion.output_scale_x = 1.0f;
    setup.motion.output_scale_y = 1.0f;
    setup.motion.invalid_value = RSF_MOTION_SENTINEL;
    setup.motion.zero_means_unwritten = 1u;
    setup.log = log;
    setup.log_user = log_user;

    started = rsf_dlss_pipeline_start(bridge.device, &setup);
    if (started != RSF_DLSS_PIPELINE_OK) {
        say("dlss bridge: pipeline did not start, result %d", (int)started);
        return 0;
    }

    memset(&tap, 0, sizeof(tap));
    tap.struct_size = sizeof(tap);
    tap.abi_version = RSF_FRAME_TAP_ABI_VERSION;
    tap.on_pass = on_pass;
    tap.on_target_draw = on_target_draw;
    tap.log = log;
    tap.log_user = log_user;
    tap.view_constant_bytes = RSF_AC7_VIEW_BUFFER_BYTES;
    /* What the tap judges sizes against. The presented size, not a size derived from the bound
       set: deriving it there made the render size an exact requirement and nothing ever matched. */
    tap.output_width = (uint32_t)output_width;
    tap.output_height = (uint32_t)output_height;

    tapped = rsf_frame_tap_install(bridge.context, &tap);
    if (tapped != RSF_FRAME_TAP_OK) {
        say("dlss bridge: frame tap not installed, result %d", (int)tapped);
        rsf_dlss_pipeline_stop();
        return 0;
    }

    bridge.started = 1;
    say("dlss bridge: running, watching for the pass that binds the reconstruction inputs");
    return 1;
}

void rsf_bridge_report(void)
{
    rsf_dlss_pipeline_status status;
    rsf_frame_tap_status tap;

    memset(&tap, 0, sizeof(tap));
    tap.struct_size = sizeof(tap);
    if (rsf_frame_tap_get_status(&tap) == RSF_FRAME_TAP_OK) {
        /* Nothing has moved since the last report, so there is nothing to say. This is what makes
           it safe to call on a timer: a report every few seconds is what turns one key press into
           an answer, and repeating identical numbers would bury the run's real events. */
        if (tap.calls_seen == bridge.reported_calls && tap.passes_seen == bridge.reported_passes &&
            bridge.evaluated == bridge.reported_evaluated) {
            return;
        }
        bridge.reported_calls = tap.calls_seen;
        bridge.reported_passes = tap.passes_seen;
        bridge.reported_evaluated = bridge.evaluated;

        say("frame tap: %lu calls seen, %lu changed a binding, %lu passes matched, render %ux%u",
            (unsigned long)tap.calls_seen, (unsigned long)tap.calls_inspected,
            (unsigned long)tap.passes_seen, tap.render_width, tap.render_height);
        say("roles recognised: motion %lu, depth %lu, exposure %lu (a pass needs all three at once)",
            (unsigned long)tap.motion_seen, (unsigned long)tap.depth_seen,
            (unsigned long)tap.exposure_seen);
    }

    say("bridge: %lu passes, %lu view reads failed, %lu not the main view, %lu without jitter, "
        "%lu evaluated, %lu refused, last result %ld",
        bridge.passes, bridge.view_read_failures, bridge.not_main_view, bridge.no_jitter,
        bridge.evaluated, bridge.refused, bridge.last_result);

    /* Whether the frame's tail was ever described. Zero draws with a watch that was set is a
       different fault from a watch that was never armed, and both look like silence otherwise. */
    say("frame tail: %lu presents watched, %lu draws described, composite %s",
        bridge.tail_frames, bridge.tail_draws,
        bridge.composite_found ? "identified" : "not identified yet");

    memset(&status, 0, sizeof(status));
    status.struct_size = sizeof(status);
    if (rsf_dlss_pipeline_get_status(&status) == RSF_DLSS_PIPELINE_OK) {
        say("pipeline: running %u, dlss supported %u, render %ux%u, output %ux%u, evaluated %llu, "
            "refused %llu",
            status.running, status.dlss_supported, status.render_width, status.render_height,
            status.output_width, status.output_height,
            (unsigned long long)status.frames_evaluated,
            (unsigned long long)status.frames_refused);

        /* Whether this is upscaling at all. The game puts its screen percentage back when a
           mission loads, and a backend fed the presented size is doing antialiasing instead. That
           is not visible in the result, which looks clean and sharp precisely because nothing was
           reconstructed from less, so it has to be said rather than seen. A mission was watched
           this way and read as a successful upscale. */
        if (tap.render_width != 0 && status.output_width != 0 &&
            tap.render_width >= status.output_width) {
            say("note: the game is rendering at the presented size, so this is antialiasing at "
                "native resolution and not upscaling. Press F8 to put the render scale back");
        }
    }
}

void rsf_bridge_request_dump(const char* prefix)
{
    if (bridge.started) {
        rsf_dlss_pipeline_request_dump(prefix);
    }
}

int rsf_bridge_running(void)
{
    return bridge.started;
}
