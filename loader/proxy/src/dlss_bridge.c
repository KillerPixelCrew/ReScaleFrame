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
#include <rescaleframe/ac7_scene_color.h>
#include <rescaleframe/constant_buffer_read.h>
#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/frame_tap.h>
#include <rescaleframe/depth_replay.h>
#include <rescaleframe/present_blit.h>
#include <rescaleframe/resource_ref.h>
#include <rescaleframe/scene_reinsert.h>

#include <stdio.h>
#include <string.h>

#include "dlss_bridge.h"
#include "overlay_host.h"

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

/* DXGI_FORMAT_R8G8B8A8_UNORM, written as a number because this file has no D3D headers.

   It is how the interface's own target is told apart from the scene's. The replayed captures in
   ac7-frame-capture.md have every scene target in the tail as B8G8R8A8 and the interface alone, on
   a transparent background, as R8G8B8A8. That is one observed difference in one game and not a
   rule about engines, which is why an input that does not match leaves the interface unpromoted
   rather than being promoted on a guess. */
#define RSF_FORMAT_R8G8B8A8_UNORM 28ul

/* Eight bit colour, in every spelling this game's tail uses.

   Written as numbers because this file has no D3D headers. The typeless entries are not pedantry:
   the composite arrives bound as `R8G8B8A8_TYPELESS`, 27, and a check for `R8G8B8A8_UNORM` alone
   missed it in every run. A view's format is whatever the view was created with, and a pooled
   target is commonly typeless. */
static int is_eight_bit_colour(unsigned long format)
{
    switch (format) {
    case 27ul: /* R8G8B8A8_TYPELESS */
    case 28ul: /* R8G8B8A8_UNORM */
    case 29ul: /* R8G8B8A8_UNORM_SRGB */
    case 87ul: /* B8G8R8A8_UNORM */
    case 88ul: /* B8G8R8X8_UNORM */
    case 90ul: /* B8G8R8A8_TYPELESS */
    case 91ul: /* B8G8R8A8_UNORM_SRGB */
        return 1;
    default:
        return 0;
    }
}

/* The R8G8B8A8 family alone, which is how the interface's own target is told from the scene's.
   The replayed capture has every scene target in the tail as B8G8R8A8 and the interface alone, on
   a transparent background, as R8G8B8A8. Same reason for the typeless entry as above. */
static int is_interface_colour(unsigned long format)
{
    return format == 27ul || format == 28ul || format == 29ul;
}

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
       The set is recognised more than once per frame and the last one wins, because on_pass
       replaces what it holds. That is not the same as choosing correctly: a briefing capture shows
       three qualifying passes whose colour is the same partial layer, while the content that is
       missing is rendered by a pass that never qualifies at all. Describing each qualifying pass
       of a few frames says how many there are and what colour each carries. */
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
    rsf_depth_replay* depth_replay[2];
    unsigned long depth_evaluations;
    unsigned long depth_candidates;
    unsigned long depth_candidate_width;
    unsigned long depth_candidate_height;
    unsigned long depth_candidate_samples;
    unsigned long geometry_draws;
    unsigned long geometry_traced;
    unsigned long depth_handover_traced;
    unsigned long depth_replay_width[2];
    unsigned long depth_replay_height[2];
    unsigned long depth_replayed;
    rsf_ac7_scene_color color_selection;
    unsigned long composed_evaluations;
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

    /* The interface's own target, and the scene colour, both taken from the frame and both held.
       The plan names them by address and the tap never dereferences them, so a reference of our own
       is what keeps that address meaning what it meant when it was learned. */
    void* interface_target;
    void* scene_color;

    /* Reinsertion proper. Off until asked for: it changes what the game draws, and a wrong
       substitution is a corrupted frame or a dead process rather than a diagnostic nobody reads. */
    rsf_reinsert* reinsert;
    int reinsert_on;
    unsigned long reinsert_frames;
    unsigned long gate_evaluates;

    /* What the overlay can ask the carrier for. See dlss_bridge.h. */
    rsf_bridge_actions actions;
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
    if (rsf_ac7_scene_color_source(&bridge.color_selection, pass->scene_color, pass->context,
                                   pass->render_width, pass->render_height)) {
        rsf_frame_tap_watch_input(bridge.color_selection.source);
    }

    /* The scene colour, kept past the frame this time. The reinsertion plan names it by address so
       the tonemap's read of it can be turned into a read of the reconstruction, and it is the same
       pooled target every frame. Replaced rather than ignored when it changes, which is what a
       render scale change or a resolution change looks like from here. */
    if (bridge.scene_color != pass->scene_color) {
        rsf_resource_release(bridge.scene_color);
        bridge.scene_color = pass->scene_color;
        rsf_resource_retain(bridge.scene_color);
    }
    (void)frame;
    (void)result;
}

/* How many geometry draws are described in full before the counters take over.

   Enough to cover a frame's translucency and stop well short of a log nobody can open. Every
   question about this path so far has been answered by one line that was not being written, and
   each of those cost a run of the game, so this writes all of them at once. */
#define RSF_GEOMETRY_TRACE_DRAWS 400u

static void on_geometry(void* user, const rsf_frame_tap_geometry* draw)
{
    unsigned int i;
    unsigned int rejects[2];
    (void)user;
    if (!bridge.started) {
        return;
    }
    /* Counted before the game-specific filter as well as after, because "no candidates" and "the
       filter rejected them all" are different answers and looked identical. */
    ++bridge.geometry_draws;
    if (!rsf_ac7_scene_depth_candidate(draw)) {
        if (bridge.geometry_traced < RSF_GEOMETRY_TRACE_DRAWS) {
            ++bridge.geometry_traced;
            say("geometry %lu: NOT a candidate. target %p %lux%lu format %lu samples %lu, depth "
                "view %p, kind %lu topology %lu, %lu elements, %lu instances",
                bridge.geometry_draws, draw->target, (unsigned long)draw->width,
                (unsigned long)draw->height, (unsigned long)draw->format,
                (unsigned long)draw->samples, draw->depth_view, (unsigned long)draw->kind,
                (unsigned long)draw->topology, (unsigned long)draw->count,
                (unsigned long)draw->instances);
        }
        return;
    }
    ++bridge.depth_candidates;
    bridge.depth_candidate_width = draw->width;
    bridge.depth_candidate_height = draw->height;
    bridge.depth_candidate_samples = draw->samples;

    for (i = 0; i < 2; ++i) {
        const uint32_t replayed = rsf_depth_replay_draw(bridge.depth_replay[i], draw);
        bridge.depth_replayed += replayed;
        rejects[i] = replayed ? 0u : rsf_depth_replay_last_reject(bridge.depth_replay[i]);
    }

    if (bridge.geometry_traced < RSF_GEOMETRY_TRACE_DRAWS) {
        ++bridge.geometry_traced;
        say("geometry %lu: candidate. target %p %lux%lu format %lu samples %lu, depth view %p, "
            "kind %lu topology %lu, %lu elements, %lu instances, vs %p. Replay targets %lux%lu "
            "and %lux%lu refused %lu and %lu",
            bridge.geometry_draws, draw->target, (unsigned long)draw->width,
            (unsigned long)draw->height, (unsigned long)draw->format,
            (unsigned long)draw->samples, draw->depth_view, (unsigned long)draw->kind,
            (unsigned long)draw->topology, (unsigned long)draw->count,
            (unsigned long)draw->instances, draw->vertex_shader,
            bridge.depth_replay_width[0], bridge.depth_replay_height[0],
            bridge.depth_replay_width[1], bridge.depth_replay_height[1],
            (unsigned long)rejects[0], (unsigned long)rejects[1]);
    }
}

static void on_input_draw(void* user, const rsf_frame_tap_target_draw* draw)
{
    (void)user;
    rsf_ac7_scene_color_draw(&bridge.color_selection, draw);
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

    /* A draw into the composite. One of its inputs is the interface's own target, which is what has
       to be promoted for the HUD to be drawn at output resolution instead of magnified with the
       scene. It is told apart by its format: every scene target in this tail is B8G8R8A8 and the
       interface alone is R8G8B8A8. Nothing else in a composite draw's inputs matches that, and an
       input that does not match leaves it unidentified rather than guessed at. */
    if (draw->watch_index == 1 && !bridge.interface_target) {
        uint32_t index;
        for (index = 0; index < draw->input_count; ++index) {
            const rsf_frame_tap_input* input = &draw->inputs[index];
            if (!input->texture || !is_interface_colour(input->format)) {
                continue;
            }
            bridge.interface_target = input->texture;
            rsf_resource_retain(bridge.interface_target);
            say("  slot %lu is the interface's own target at %lux%lu, so the HUD can be drawn at "
                "output resolution rather than magnified with the scene",
                (unsigned long)input->slot, (unsigned long)input->width,
                (unsigned long)input->height);
            break;
        }
    }

    /* What the back buffer draw reads is the composite. At a reduced render scale it reads exactly
       one resource, so anything else is a different tail than the one the capture describes, and
       taking the first input regardless would name the wrong texture and describe the wrong pass.
       Refusing here costs one game run and is the difference between an answer and a guess. */
    if (draw->watch_index != 0 || bridge.composite) {
        return;
    }
    /* Pick the composite out of what is bound, rather than expecting it to be alone.

       The capture shows the frame ending in one draw that reads a single composite, and this used
       to require exactly that. A running game does not oblige: D3D11 leaves shader resource slots
       bound until something replaces them, so the same draw arrives here with seven inputs, of
       which one is read. frame_tap.h says as much, that bindings establish possible reads and not
       reads, and this is what that costs when ignored. The requirement matched nothing on the
       briefing screen and the tail was never identified in any run.

       What the composite is, among those seven: the picture that goes to the back buffer, so it is
       eight bit colour because it is past the tonemap, and it is the size of a picture rather than
       a bloom mip. The rest of that draw's bindings are the bloom chain at 256x144 and 128x72, a
       63x63 dirt or lens texture, the pre-tonemap scene colour in half float, and the velocity
       target. Size and format between them name it without guessing. */
    const rsf_frame_tap_input* composite = NULL;
    {
        uint32_t index;
        for (index = 0; index < draw->input_count; ++index) {
            const rsf_frame_tap_input* input = &draw->inputs[index];
            if (!input->texture || !is_eight_bit_colour(input->format)) {
                continue;
            }
            /* Half the target's height is a wide margin: the composite is either the size of the
               back buffer or exactly half it at a reduced render scale, and every mip and bar in
               this draw is far smaller. */
            if (input->height * 2u < draw->target_height) {
                continue;
            }
            composite = input;
            break;
        }
    }
    if (!composite) {
        return;
    }
    /* Our own debug blit also draws over the back buffer, from inside the Present hook, and nothing
       about its shape distinguishes it from the game's last draw. Taking it would point the
       reinsertion at the reconstruction's own output. */
    if (composite->texture == rsf_dlss_pipeline_output_texture()) {
        return;
    }
    bridge.composite = composite->texture;
    bridge.composite_found = 1;
    rsf_resource_retain(bridge.composite);
    rsf_frame_tap_watch_target(1, bridge.composite, RSF_TAIL_COMPOSITE_DRAWS);
    say("  slot %lu, %lux%lu format %lu, is the composite the scene has to be replaced in. "
        "Watching it: the draw into it that reads scene colour is where the reconstruction goes "
        "back",
        (unsigned long)composite->slot, (unsigned long)composite->width,
        (unsigned long)composite->height, (unsigned long)composite->format);
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
        /* The watch stops; the references do not. The composite and the interface target are what
           the reinsertion plan names, and an engine pooled target is not made harder to reuse by
           one more reference the way a swap chain buffer is. */
        rsf_frame_tap_watch_target(1, NULL, 0);
        say("frame tail: done looking, %lu draws described, composite %s, interface target %s",
            bridge.tail_draws, bridge.composite ? "found" : "not found",
            bridge.interface_target ? "found" : "not found");
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
    frame.scene_color = rsf_ac7_scene_color_selected(&bridge.color_selection, bridge.held_color);
    if (frame.scene_color != bridge.held_color) {
        if (bridge.composed_evaluations < 4) {
            say("composed scene colour: %p -> %p at %lux%lu", bridge.held_color,
                frame.scene_color, bridge.held_width, bridge.held_height);
        }
        ++bridge.composed_evaluations;
    }
    frame.depth = bridge.held_depth;
    if (frame.scene_color != bridge.held_color) {
        unsigned int i;
        for (i = 0; i < 2; ++i) {
            void* selected =
                rsf_depth_replay_selected(bridge.depth_replay[i], context, bridge.held_depth,
                                          bridge.color_selection.composed_layer);
            if (selected != bridge.held_depth) {
                frame.depth = selected;
            }
        }
        if (frame.depth != bridge.held_depth) {
            if (bridge.depth_evaluations < 4) {
                say("translucent depth: %p -> %p at %lux%lu", bridge.held_depth, frame.depth,
                    bridge.held_width, bridge.held_height);
            }
            ++bridge.depth_evaluations;
        } else if (bridge.depth_handover_traced < 8) {
            /* Both sides of the comparison, here rather than in the periodic report.

               The report runs on a timer and end_frame clears the layer, the depth and the context
               the moment the frame finishes, so every field read there was null and said nothing.
               This is the one place where both sides exist at once. */
            unsigned int slot;
            ++bridge.depth_handover_traced;
            for (slot = 0; slot < 2; ++slot) {
                rsf_depth_replay_detail detail;
                memset(&detail, 0, sizeof(detail));
                rsf_depth_replay_get_detail(bridge.depth_replay[slot], &detail);
                say("translucent depth: replay %u holds layer %p source %p context %p, %lu draws, "
                    "refused %lu; the frame offers layer %p depth %p context %p",
                    slot, detail.layer, detail.source, detail.context, (unsigned long)detail.draws,
                    (unsigned long)detail.refused, bridge.color_selection.composed_layer,
                    bridge.held_depth, context);
            }
        }
    }
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

/* Called by the frame tap when the game binds the composite, before that binding is forwarded.

   This is where the reconstruction has to run once the result is being reinserted. The scene is
   finished by now, the post chain has not read it yet, and the tonemap that follows within the same
   pass is the draw whose scene colour is about to be substituted. Evaluating at Present instead,
   which is what the debug view does, would put the reconstruction a whole frame behind the grade
   and the interface drawn over it.

   The price of being here rather than at Present is that the game is midway through its frame and
   will not rebind what it believes is still bound. Streamline says it does not restore state, so
   the whole pipeline is saved and put back around the evaluate. */
static void on_gate(void* user, void* context, void* texture)
{
    rsf_d3d11_state state;

    (void)user;
    (void)texture;
    if (!bridge.have_held || !context) {
        return;
    }
    if (!rsf_d3d11_state_save(context, &state)) {
        return;
    }
    evaluate_held(context);
    rsf_d3d11_state_restore(context, &state);
    ++bridge.gate_evaluates;
}

/* The debug view: draw the reconstruction over the finished frame.

   It replaces a graded image that has an interface on it with an ungraded one that does not, so it
   looks wrong in brightness and has no HUD even when the reconstruction is perfect. That is the
   price of being able to see motion at all, and it is a debug view rather than a step towards how
   this should work. */
static void show_result(void* swapchain)
{
    void* output;

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

/* Everything the panel displays, gathered from where it actually lives.

   It is filled every frame the panel is open rather than kept as state, because a number the panel
   shows and a number the bridge holds disagreeing is the failure this is meant to catch, not to
   introduce. */
static void fill_overlay_stats(rsf_overlay_stats* stats)
{
    rsf_dlss_pipeline_status pipeline;
    rsf_frame_tap_status tap;

    memset(stats, 0, sizeof(*stats));
    stats->struct_size = sizeof(*stats);

    memset(&pipeline, 0, sizeof(pipeline));
    pipeline.struct_size = sizeof(pipeline);
    if (rsf_dlss_pipeline_get_status(&pipeline) == RSF_DLSS_PIPELINE_OK) {
        stats->backend_loaded = pipeline.running;
        stats->backend_supported = pipeline.dlss_supported;
        stats->render_width = pipeline.render_width;
        stats->render_height = pipeline.render_height;
        stats->output_width = pipeline.output_width;
        stats->output_height = pipeline.output_height;
        stats->frames_evaluated = (uint32_t)pipeline.frames_evaluated;
        stats->frames_refused = (uint32_t)pipeline.frames_refused;
        stats->last_result = (int32_t)pipeline.last_result;
    }
    stats->backend_name = "DLSS";

    /* Why nothing is happening, in the order the pipeline actually fails. A backend that is running
       and evaluating nothing is the normal outcome of an unjittered projection, and saying so is
       the entire reason this field exists. */
    if (!bridge.started) {
        stats->refusal_reason = "not started, press F8";
    } else if (!stats->backend_supported) {
        stats->refusal_reason = "the driver did not accept DLSS";
    } else if (bridge.passes == 0) {
        stats->refusal_reason = "no pass has bound the reconstruction inputs yet";
    } else if (bridge.no_jitter > 0 && bridge.evaluated == 0) {
        stats->refusal_reason = "the projection carries no jitter, press F9";
    } else if (bridge.not_main_view > 0 && bridge.evaluated == 0) {
        stats->refusal_reason = "the view read was not the main view";
    }

    memset(&tap, 0, sizeof(tap));
    tap.struct_size = sizeof(tap);
    if (rsf_frame_tap_get_status(&tap) == RSF_FRAME_TAP_OK) {
        stats->have_motion = tap.motion_seen > 0;
        stats->have_depth = tap.depth_seen > 0;
        stats->have_exposure = tap.exposure_seen > 0;
        if (stats->render_width == 0) {
            stats->render_width = tap.render_width;
            stats->render_height = tap.render_height;
        }
    }
    stats->have_scene_color = bridge.scene_color != NULL || bridge.held_color != NULL;
    stats->motion_decoded = bridge.evaluated > 0;
    stats->jitter_active = bridge.held_camera.has_jitter;
    stats->jitter_pixels[0] = bridge.held_camera.jitter_pixels[0];
    stats->jitter_pixels[1] = bridge.held_camera.jitter_pixels[1];
    stats->frames_presented = bridge.frames_shown;
    stats->enabled = (uint32_t)(bridge.reinsert_on || bridge.show);
    stats->debug_view_on = (uint32_t)bridge.show;
    stats->reinsert_on = (uint32_t)bridge.reinsert_on;
    /* What rsf_bridge_toggle_reinsert would refuse on, asked before the click rather than after.
       The composite has to have been identified and a reconstruction has to exist. */
    stats->reinsert_available =
        (uint32_t)(bridge.started && bridge.composite_found && bridge.scene_color != NULL &&
                   bridge.evaluated > 0);
    stats->render_scale_percent =
        bridge.actions.render_scale_percent ? (uint32_t)bridge.actions.render_scale_percent() : 0u;
    stats->captures_written =
        bridge.actions.capture_count ? (uint32_t)bridge.actions.capture_count() : 0u;
}

/* Lay the panel out and draw it, and report what was clicked without acting on it.

   Nothing here applies an intent yet. The panel can already change quality and toggle
   reconstruction in its own model, and wiring those to the bridge means a settings change crossing
   from the message thread to the render thread at a defined point, which is the open review finding
   about F7 and F8 and is not made better by adding a third way in. So this says what was asked for
   and leaves the hotkeys as the way to ask. */
static void overlay_tick(void* swapchain)
{
    rsf_overlay_stats stats;
    rsf_overlay_intent intent;
    if (!bridge.log) {
        return;
    }
    /* The presenting chain selects the overlay's device. The observer's first allocation can
       belong to a helper device, including one created by capture support. */
    if (!rsf_overlay_host_start(swapchain, bridge.log, bridge.log_user) ||
        !rsf_overlay_host_visible()) {
        return;
    }

    fill_overlay_stats(&stats);
    memset(&intent, 0, sizeof(intent));
    intent.struct_size = sizeof(intent);
    if (!rsf_overlay_host_present(swapchain, &stats, &intent)) {
        return;
    }
    /* Applied here, on the render thread, at the point in the frame the overlay was drawn from.
       That is the boundary the review finding asks for: the panel records what was clicked and the
       change happens where the rendering already is, rather than from the message thread while a
       frame is in flight. */
    if (intent.start_requested) {
        if (bridge.actions.start_backend) {
            bridge.actions.start_backend();
        } else {
            say("overlay: nothing is registered to start the backend");
        }
    }
    if (intent.scale_requested && intent.scale_percent > 0) {
        if (bridge.actions.set_render_scale) {
            bridge.actions.set_render_scale(intent.scale_percent);
        } else {
            say("overlay: nothing is registered to set the render scale");
        }
    }
    if (intent.debug_view_changed && (intent.debug_view != 0) != (bridge.show != 0)) {
        rsf_bridge_toggle_display();
    }
    if (intent.reinsert_changed && (intent.reinsert != 0) != (bridge.reinsert_on != 0)) {
        rsf_bridge_toggle_reinsert();
    }
    if (intent.dump_requested) {
        if (bridge.actions.trigger_dump) {
            bridge.actions.trigger_dump();
        } else {
            say("overlay: nothing is registered to write a dump");
        }
    }
    if (intent.enabled_changed) {
        /* The master enable is the pair of switches above it in the panel, and there is no third
           thing for it to mean. Said rather than quietly doing nothing. */
        say("overlay: the enable toggle does not act on its own. Use the debug view and the "
            "reinsertion switches");
    }
    if (intent.capture_requested) {
        if (bridge.actions.trigger_capture) {
            bridge.actions.trigger_capture();
        } else {
            say("overlay: no capture support was registered, so there is nothing to capture with");
        }
    }
    if (intent.quality_changed) {
        /* Quality selects a render size, so changing it means rebuilding the backend rather than
           setting a value. Not done here yet, and said rather than silently ignored. */
        say("overlay: quality %u was asked for. It needs the backend rebuilt, which is not wired "
            "up yet",
            (unsigned)intent.quality);
    }
}

/* Called before the game's own Present, from the observer. */
static void on_present(void* user, void* swapchain)
{
    (void)user;
    if (bridge.frames_described < 4 && bridge.pass_in_frame > 0) {
        say("frame ended after %lu qualifying passes", bridge.pass_in_frame);
        ++bridge.frames_described;
    }
    bridge.pass_in_frame = 0;

    if (bridge.started) {
        watch_tail(swapchain);
        if (bridge.reinsert_on) {
            /* The evaluate already happened, at the gate, where it has to happen for the result to
               reach the game's own tonemap. All that is left is to let go of the frame's inputs and
               to close the gates so the next frame opens them again. */
            ++bridge.reinsert_frames;
            release_held();
            rsf_frame_tap_end_frame();
        } else {
            /* The frame is finished here, which is the whole reason the evaluate waits for it when
               the result is only being drawn over the top. */
            evaluate_held(bridge.context);
        }
    }

    rsf_ac7_scene_color_end_frame(&bridge.color_selection);
    rsf_depth_replay_end_frame(bridge.depth_replay[0]);
    rsf_depth_replay_end_frame(bridge.depth_replay[1]);
    show_result(swapchain);

    /* Last, so the panel is drawn over the finished frame and over the debug view when that is on.
       It takes no part in the reconstruction and is never one of its inputs. */
    overlay_tick(swapchain);
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

void rsf_bridge_set_log(rsf_bridge_log_fn log, void* log_user)
{
    bridge.log = log;
    bridge.log_user = log_user;
}

void rsf_bridge_set_actions(const rsf_bridge_actions* actions)
{
    if (actions) {
        bridge.actions = *actions;
    } else {
        memset(&bridge.actions, 0, sizeof(bridge.actions));
    }
}

/* Stop substituting and put the frame back the way the game draws it. */
static void stop_reinsert(void)
{
    rsf_frame_tap_set_plan(NULL);
    rsf_frame_tap_end_frame();
    bridge.reinsert_on = 0;
    say("reinsert: off, the game draws its own frame again");
}

void rsf_bridge_toggle_reinsert(void)
{
    rsf_reinsert_setup setup;
    rsf_reinsert_frame_tail tail;
    rsf_frame_tap_plan plan;
    rsf_dlss_pipeline_status pipeline;
    rsf_reinsert_result prepared;
    void* reconstruction;

    if (bridge.reinsert_on) {
        stop_reinsert();
        return;
    }
    if (!bridge.started) {
        say("reinsert: nothing to reinsert, the backend is not running");
        return;
    }

    /* Every one of these is a thing the frame had to say for itself, and a missing one names which
       run to do again rather than leaving a silent no-op. */
    if (!bridge.composite) {
        say("reinsert: the composite has not been identified yet, so there is nowhere to put the "
            "result. It is learned from the draw into the back buffer in the first few frames");
        return;
    }
    if (!bridge.scene_color) {
        say("reinsert: no scene colour has been seen yet, so nothing has been reconstructed");
        return;
    }
    if (bridge.held_width == 0 || bridge.held_height == 0) {
        say("reinsert: the render resolution is not known yet");
        return;
    }

    memset(&pipeline, 0, sizeof(pipeline));
    pipeline.struct_size = sizeof(pipeline);
    if (rsf_dlss_pipeline_get_status(&pipeline) != RSF_DLSS_PIPELINE_OK || !pipeline.running) {
        say("reinsert: the pipeline is not running");
        return;
    }
    reconstruction = rsf_dlss_pipeline_output_texture();
    if (!reconstruction) {
        say("reinsert: there is no reconstruction to put back");
        return;
    }

    if (!bridge.reinsert) {
        memset(&setup, 0, sizeof(setup));
        setup.struct_size = sizeof(setup);
        setup.abi_version = RSF_REINSERT_ABI_VERSION;
        setup.device = bridge.device;
        setup.output_width = pipeline.output_width;
        setup.output_height = pipeline.output_height;
        setup.log = bridge.log;
        setup.log_user = bridge.log_user;
        if (rsf_reinsert_create(&setup, &bridge.reinsert) != RSF_REINSERT_OK) {
            say("reinsert: could not be created");
            return;
        }
    }

    memset(&tail, 0, sizeof(tail));
    tail.struct_size = sizeof(tail);
    tail.composite = bridge.composite;
    tail.interface_target = bridge.interface_target;
    tail.scene_color = bridge.scene_color;
    tail.reconstruction = reconstruction;
    tail.render_width = (uint32_t)bridge.held_width;
    tail.render_height = (uint32_t)bridge.held_height;
    prepared = rsf_reinsert_prepare(bridge.reinsert, &tail);
    if (prepared == RSF_REINSERT_ERROR_NOT_SCALED) {
        say("reinsert: press F9 to put the render scale back first, there is nothing to upscale");
        return;
    }
    if (prepared != RSF_REINSERT_OK) {
        say("reinsert: the replacements could not be prepared, result %d", (int)prepared);
        return;
    }

    memset(&plan, 0, sizeof(plan));
    plan.struct_size = sizeof(plan);
    if (rsf_reinsert_fill_plan(bridge.reinsert, &plan) != RSF_REINSERT_OK) {
        say("reinsert: the plan could not be built");
        return;
    }
    plan.on_gate = on_gate;

    if (rsf_frame_tap_set_plan(&plan) != RSF_FRAME_TAP_OK) {
        say("reinsert: the frame tap refused the plan");
        return;
    }
    bridge.reinsert_on = 1;
    /* Deliberately not "reinsertion works". The substitutions are in place and the game will draw
       its own tail over the reconstruction; whether the result is right is a thing to look at. */
    say("reinsert: on. The scene is reconstructed before the game's tonemap, and the grade and the "
        "interface are the game's own. Press F6 again to stop");
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
    tap.on_input_draw = on_input_draw;
    tap.on_geometry = on_geometry;
    tap.log = log;
    tap.log_user = log_user;
    tap.view_constant_bytes = RSF_AC7_VIEW_BUFFER_BYTES;
    /* What the tap judges sizes against. The presented size, not a size derived from the bound
       set: deriving it there made the render size an exact requirement and nothing ever matched. */
    tap.output_width = (uint32_t)output_width;
    tap.output_height = (uint32_t)output_height;

    /* Called from the setup worker. Allocate once, before hooks can replay a draw. The recorded
       full/50% sizes are supported; a different extent safely uses the original depth. */
    bridge.depth_replay[0] =
        rsf_depth_replay_create(bridge.device, (uint32_t)output_width, (uint32_t)output_height);
    bridge.depth_replay[1] = rsf_depth_replay_create(bridge.device, (uint32_t)output_width / 2,
                                                     (uint32_t)output_height / 2);
    bridge.depth_replay_width[0] = output_width;
    bridge.depth_replay_height[0] = output_height;
    bridge.depth_replay_width[1] = output_width / 2;
    bridge.depth_replay_height[1] = output_height / 2;
    say("translucent depth: replay targets built at %lux%lu and %lux%lu", output_width,
        output_height, output_width / 2, output_height / 2);
    if (!bridge.depth_replay[0] || !bridge.depth_replay[1]) {
        say("translucent depth: a startup target could not be prepared; that size will fall back");
    }

    tapped = rsf_frame_tap_install(bridge.context, &tap);
    if (tapped != RSF_FRAME_TAP_OK) {
        rsf_depth_replay_destroy(bridge.depth_replay[0]);
        rsf_depth_replay_destroy(bridge.depth_replay[1]);
        bridge.depth_replay[0] = NULL;
        bridge.depth_replay[1] = NULL;
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

    say("translucent depth: %lu candidate draws, %lu replayed, %lu selected evaluations, last "
        "refusals %lu and %lu, last candidate %lux%lu samples %lu",
        bridge.depth_candidates, bridge.depth_replayed, bridge.depth_evaluations,
        (unsigned long)rsf_depth_replay_last_reject(bridge.depth_replay[0]),
        (unsigned long)rsf_depth_replay_last_reject(bridge.depth_replay[1]),
        bridge.depth_candidate_width, bridge.depth_candidate_height,
        bridge.depth_candidate_samples);
    say("translucent depth: %lu geometry draws reached the hook, %lu of them candidates",
        bridge.geometry_draws, bridge.depth_candidates);
    {
        unsigned int slot;
        for (slot = 0; slot < 2; ++slot) {
            rsf_depth_replay_detail detail;
            memset(&detail, 0, sizeof(detail));
            rsf_depth_replay_get_detail(bridge.depth_replay[slot], &detail);
            say("translucent depth: replay %u built %lux%lu, last draw %lux%lu samples %lu, depth "
                "view format %lu dimension %lu flags 0x%lx, its texture %lux%lu format %lu samples "
                "%lu",
                slot, bridge.depth_replay_width[slot], bridge.depth_replay_height[slot],
                (unsigned long)detail.draw_width, (unsigned long)detail.draw_height,
                (unsigned long)detail.draw_samples, (unsigned long)detail.dsv_format,
                (unsigned long)detail.dsv_dimension, (unsigned long)detail.dsv_flags,
                (unsigned long)detail.source_width, (unsigned long)detail.source_height,
                (unsigned long)detail.source_format, (unsigned long)detail.source_samples);
        }
    }
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
    say("frame tail: %lu presents watched, %lu draws described, composite %s, interface target %s",
        bridge.tail_frames, bridge.tail_draws,
        bridge.composite_found ? "identified" : "not identified yet",
        bridge.interface_target ? "identified" : "not identified");

    /* What the substitution is actually doing, which the tap counts because it is the only thing
       that sees every binding. Reinsertion on with nothing redirected means the plan names a
       texture the frame never binds, and that is invisible in the picture: the game simply draws
       what it always drew. */
    if (bridge.reinsert_on) {
        say("reinsert: on, %lu frames, %lu evaluates at the gate, %lu bindings substituted, "
            "%lu targets redirected, %lu gates opened",
            bridge.reinsert_frames, bridge.gate_evaluates, (unsigned long)tap.inputs_substituted,
            (unsigned long)tap.targets_redirected, (unsigned long)tap.gates_opened);
    }

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
