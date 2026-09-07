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
#include <rescaleframe/ac7_ui_rules.h>
#include <rescaleframe/fullscreen_pass.h>
#include <rescaleframe/resource_ref.h>
#include <rescaleframe/scene_reinsert.h>
#include <rescaleframe/ui_identify.h>
#include <rescaleframe/ui_layer.h>

#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
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
/* How many times a restake will look again before settling for what it can see. A video is a few
   seconds and each look is 32 frames, so this outlasts one without spinning forever if a screen
   genuinely has no interface. */
#define RSF_TAIL_MAX_RELOOKS 12ul
/* How many draws the interface hunt looks at per screen.

   Large, because the budget is spent by every draw that reads the interface and most of them are
   not squashes: the interface composites itself over several 1920x1080 passes before anything
   downsamples it. Forty-eight was enough on a menu and ran out on a briefing before a squash
   appeared, which reads exactly like a screen whose interface cannot be found. The matching itself
   is a comparison against descriptions the tap already holds, so looking is nearly free and only
   the reporting needs restraint. */
#define RSF_UI_HUNT_DRAWS 20000u
/* How many of those to describe in the log, which is the part that actually costs something. */
#define RSF_UI_HUNT_LOGGED 24u
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
    /* Indices drawn into the separate translucency layer, this frame and the last completed one.
       The second is kept so the report and the panel have something to show: the live one is zero
       for most of a frame and would read as "nothing there" whenever it was asked. */
    unsigned long translucent_indices;
    unsigned long translucent_indices_last;
    unsigned long translucent_draws;
    unsigned long translucent_draws_last;
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
    /* Whether the tail is being looked for again because the plan went stale, and how many times
       that has happened. The plan matches textures by address, and holding a reference keeps a
       texture alive without keeping it in use: the engine's render target pool is free to give the
       composite role to a different allocation, after which every substitution silently stops
       matching and reinsertion does nothing while still reporting itself as on. */
    unsigned long redirects_seen;
    unsigned long redirect_stall;
    unsigned long tail_restakes;
    unsigned long tail_relooks;
    int tail_restaking;
    /* The surfaces the interface is composited into, found by the shape hunt rather than by the
       tail's format rule. Retained, because the plan names them by address. */
    void* interface_targets[RSF_REINSERT_MAX_INTERFACE_TARGETS];
    uint32_t interface_target_count;
    unsigned long hunt_logged;
    /* The presented size, kept here so a per-draw callback can judge against it without asking the
       pipeline for its status on every draw. */
    unsigned long output_width;
    unsigned long output_height;
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

    /* Interface identification. The registry holds what each pipeline object turned out to be, the
       counts are what a run reports, and neither changes a pixel: M1 is the milestone that answers
       which draws are the interface, and nothing acts on the answer yet. */
    rsf_ui_registry* ui;
    int ui_classify;
    unsigned long ui_class_counts[7];
    unsigned long ui_candidate_draws;
    /* The extent the interface was rasterized at, and the extent it was drawn into. Two numbers
       rather than a screen name, because which screen the game is on is M3's question and claiming
       it now would be inventing it. */
    unsigned long ui_widget_extent[2];
    unsigned long ui_layer_extent[2];
    unsigned long ui_reported_counts[7];
    unsigned long ui_traced;

    /* Extraction: the layer the interface is diverted into and the pass that puts it back. Off
       until asked for, because it changes the picture and everything above it does not. */
    /* The texture the frame ends in, refreshed every present.
     *
     * Distinct from `back_buffer`, which the tail walk owns and lets go of after thirty-two frames
     * because holding a reference to it makes ResizeBuffers fail. Classification needs to know the
     * frame's own target for the whole run, not for the first half second, and reading the tail
     * walk's field instead is why the first extraction run classified zero Slate draws.
     *
     * Held without a reference on purpose: the swap chain owns it and this is only ever compared,
     * never used. It is refreshed before anything reads it, so a stale value cannot outlive a
     * resize by more than the present that discovers it. */
    void* present_target;

    rsf_ui_layer* layer;
    rsf_fullscreen_pass* composite_pass;
    int ui_extract;
    int ui_extract_failed;
    unsigned long ui_composites;
} bridge;

/* Declared here because the resource creation hooks below are defined before it and have things
   worth saying. */
static void say(const char* format, ...);

/* Publish the registry's sets to the tap, so its prefilter has something to match. Called after any
   change, which is rare: pipeline objects are created in bursts at load and then not at all. */
static void publish_ui_candidates(void)
{
    void* merged_layouts[128];
    uint32_t slate_count = 0;
    uint32_t canvas_count = 0;
    void* const* slate = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_SLATE_LAYOUT, &slate_count);
    void* const* canvas = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_CANVAS_LAYOUT, &canvas_count);
    uint32_t merged = 0;
    for (uint32_t index = 0; index < slate_count && merged < 128; ++index) {
        merged_layouts[merged++] = slate[index];
    }
    for (uint32_t index = 0; index < canvas_count && merged < 128; ++index) {
        merged_layouts[merged++] = canvas[index];
    }

    uint32_t widget_count = 0;
    void* const* widgets = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_WIDGET_TARGET, &widget_count);
    uint32_t forced_count = 0;
    void* const* forced = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_FORCE_SHADER, &forced_count);

    rsf_frame_tap_candidates candidates;
    memset(&candidates, 0, sizeof(candidates));
    candidates.struct_size = sizeof(candidates);
    candidates.layouts = merged_layouts;
    candidates.layout_count = merged;
    candidates.widget_targets = widgets;
    candidates.widget_target_count = widget_count;
    candidates.shaders = forced;
    candidates.shader_count = forced_count;
    rsf_frame_tap_set_candidates(&candidates);
}

/* A vertex declaration was created. Name it if the game's own rule recognises it.

   The address is forgotten first, always. D3D11 hands out a released address again immediately, and
   an entry that outlived its object would make this confidently wrong about a live one. */
static void on_layout_created(void* user, void* layout, const rsf_observer_layout_element* elements,
                              uint32_t copied, uint32_t count)
{
    rsf_ac7_layout_element facts[RSF_AC7_UI_MAX_LAYOUT_ELEMENTS];
    uint32_t index;
    (void)user;
    if (!bridge.ui || !layout) {
        return;
    }
    rsf_ui_registry_forget(bridge.ui, layout);
    if (copied != count || count > RSF_AC7_UI_MAX_LAYOUT_ELEMENTS) {
        return;
    }
    for (index = 0; index < copied; ++index) {
        facts[index].semantic_index = elements[index].semantic_index;
        facts[index].format = elements[index].format;
        facts[index].input_slot = elements[index].input_slot;
        facts[index].byte_offset = elements[index].byte_offset;
        facts[index].per_instance = elements[index].per_instance;
    }
    switch (rsf_ac7_ui_classify_layout(facts, copied)) {
    case RSF_AC7_LAYOUT_SLATE:
    case RSF_AC7_LAYOUT_SLATE_INSTANCED:
        rsf_ui_registry_add(bridge.ui, RSF_UI_SET_SLATE_LAYOUT, layout);
        publish_ui_candidates();
        break;
    case RSF_AC7_LAYOUT_CANVAS:
        rsf_ui_registry_add(bridge.ui, RSF_UI_SET_CANVAS_LAYOUT, layout);
        publish_ui_candidates();
        break;
    default:
        break;
    }
}

/* Shader hashes a setting named, in either direction, resolved to pointers as the game creates
   them. Small fixed arrays: naming more than a handful by hand is not a thing anyone does, and a
   longer list would mean the rules are wrong in a way a list cannot fix. */
#define RSF_UI_MAX_NAMED 16u
static unsigned long ui_forced_hashes[RSF_UI_MAX_NAMED];
static unsigned long ui_skipped_hashes[RSF_UI_MAX_NAMED];
static unsigned int ui_forced_count;
static unsigned int ui_skipped_count;
static unsigned long ui_hashes_seen;

/* Shader pointer to hash, so a trace line can name the shader that made a draw.
 *
 * Without this the override lists are unusable: the hash is known only at creation, the draw report
 * carries only a pointer, and nobody can name in a settings file a number they were never shown. A
 * bounded open-addressed table, keyed by pointer, overwriting on collision because a stale entry is
 * a wrong name in a diagnostic and never a wrong picture. */
#define RSF_UI_HASH_SLOTS 4096u
static struct {
    void* shader;
    unsigned long hash;
} ui_hash_table[RSF_UI_HASH_SLOTS];

static unsigned int ui_hash_slot(const void* shader)
{
    /* Fibonacci hashing on the pointer. Addresses are aligned, so the low bits are zeros and using
       them directly would pile every shader into a fraction of the table. */
    unsigned long long key = (unsigned long long)(size_t)shader;
    key *= 0x9E3779B97F4A7C15ull;
    return (unsigned int)((key >> 52) & (RSF_UI_HASH_SLOTS - 1u));
}

static void ui_remember_hash(void* shader, unsigned long hash)
{
    unsigned int slot = ui_hash_slot(shader);
    unsigned int probe;
    for (probe = 0; probe < 8u; ++probe) {
        const unsigned int index = (slot + probe) & (RSF_UI_HASH_SLOTS - 1u);
        if (ui_hash_table[index].shader == NULL || ui_hash_table[index].shader == shader) {
            ui_hash_table[index].shader = shader;
            ui_hash_table[index].hash = hash;
            return;
        }
    }
    ui_hash_table[slot].shader = shader;
    ui_hash_table[slot].hash = hash;
}

static unsigned long ui_hash_of(const void* shader)
{
    unsigned int slot;
    unsigned int probe;
    if (!shader) {
        return 0;
    }
    slot = ui_hash_slot(shader);
    for (probe = 0; probe < 8u; ++probe) {
        const unsigned int index = (slot + probe) & (RSF_UI_HASH_SLOTS - 1u);
        if (ui_hash_table[index].shader == shader) {
            return ui_hash_table[index].hash;
        }
        if (ui_hash_table[index].shader == NULL) {
            break;
        }
    }
    return 0;
}

static void ui_forget_hash(void* shader)
{
    unsigned int slot = ui_hash_slot(shader);
    unsigned int probe;
    for (probe = 0; probe < 8u; ++probe) {
        const unsigned int index = (slot + probe) & (RSF_UI_HASH_SLOTS - 1u);
        if (ui_hash_table[index].shader == shader) {
            ui_hash_table[index].shader = NULL;
            ui_hash_table[index].hash = 0;
            return;
        }
    }
}

/* Every shader the game creates, hashed once, and matched against what the settings named.
 *
 * The hash is the only stable name a shader has: its pointer is reused, its bytecode is borrowed
 * for the length of the creation call, and nothing else about it survives. Naming one is the escape
 * hatch for a run where the rules are wrong about a particular draw and a rebuild is too slow, which
 * is what SpecialK's HUD registry is for and why it is worth carrying. */
static void on_shader_created(void* user, void* shader, uint32_t stage, const void* bytecode,
                              uint32_t bytes)
{
    unsigned long hash;
    unsigned int index;
    (void)user;
    (void)stage;
    if (!bridge.ui || !shader) {
        return;
    }
    rsf_ui_registry_forget(bridge.ui, shader);
    ui_forget_hash(shader);
    hash = (unsigned long)rsf_ui_shader_hash(bytecode, bytes);
    ui_remember_hash(shader, hash);
    ++ui_hashes_seen;
    for (index = 0; index < ui_forced_count; ++index) {
        if (ui_forced_hashes[index] == hash) {
            rsf_ui_registry_add(bridge.ui, RSF_UI_SET_FORCE_SHADER, shader);
            publish_ui_candidates();
            say("ui: shader 0x%08lx named by the settings as interface", hash);
            return;
        }
    }
    for (index = 0; index < ui_skipped_count; ++index) {
        if (ui_skipped_hashes[index] == hash) {
            rsf_ui_registry_add(bridge.ui, RSF_UI_SET_SKIP_SHADER, shader);
            say("ui: shader 0x%08lx named by the settings as not interface", hash);
            return;
        }
    }
}

/* A texture was created, so whatever used to live at that address does not any more.
 *
 * This is the only thing the texture hook does now, and it is not a small thing: a converter target
 * recorded here and released later would otherwise leave the registry naming a live texture that
 * is something else entirely. Which textures are converter targets is settled at the draw, by
 * watching Slate write into one, for the reasons recorded there. */
static void on_texture_created(void* user, void* texture, uint32_t width, uint32_t height,
                               uint32_t format, uint32_t mip_levels, uint32_t array_size,
                               uint32_t sample_count, uint32_t bind_flags, uint32_t misc_flags)
{
    (void)user;
    (void)width;
    (void)height;
    (void)format;
    (void)mip_levels;
    (void)array_size;
    (void)sample_count;
    (void)bind_flags;
    (void)misc_flags;
    if (!bridge.ui || !texture) {
        return;
    }
    rsf_ui_registry_forget(bridge.ui, texture);
}

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
    /* What tells the briefing relief apart from a tracer, summed here because this is the only
       place that sees every draw into the layer. Instances multiply it: a hundred instanced
       billboards are a hundred billboards' worth of geometry however few draws they took. */
    ++bridge.translucent_draws;
    bridge.translucent_indices +=
        (unsigned long)draw->count * (draw->instances ? (unsigned long)draw->instances : 1ul);

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
/* A draw that reads the interface, found by the shape the binary states rather than by a format
   rule. What matters is the target it writes into and at what size: that is the surface the
   interface is composited into, and if it is at render resolution then a 1920x1080 interface is
   being squashed into it and stretched back out by the frame's last draw.

   Says which slot carried it, because a texture of that shape bound in some other slot and not read
   would be the same false positive that every earlier identification fell for. */
/* Every draw the tap's prefilter let through, classified and counted. Nothing else.

   This is what M1 is for: a run says how many draws on each screen are the interface and by which
   producer, and the number that matters most is how many came back UNKNOWN. An UNKNOWN is a draw
   that looked like the interface and matched no rule, and diverting on a guess is exactly the
   mistake this frame has made four times. */
/* Translate a tap report into the game's own facts and ask its rule what the draw is.
 *
 * Shared by the report, which only counts, and the verdict, which decides whether the draw moves.
 * Sharing matters: a run that reports one classification and acts on another would be describing a
 * frame nobody rendered. */
static rsf_ac7_draw_class classify_candidate(const rsf_frame_tap_target_draw* draw)
{
    rsf_ac7_draw_facts facts;
    rsf_ac7_ui_registry rules;
    rsf_ac7_draw_input inputs[RSF_AC7_UI_MAX_INPUTS];
    uint32_t slate_count = 0;
    uint32_t canvas_count = 0;
    uint32_t widget_count = 0;
    uint32_t forced_count = 0;
    uint32_t skip_count = 0;
    uint32_t index;
    uint32_t used = 0;
    rsf_ac7_draw_class verdict;
    if (!draw || !bridge.ui) {
        return RSF_AC7_DRAW_SCENE;
    }

    memset(&rules, 0, sizeof(rules));
    rules.struct_size = sizeof(rules);
    rules.slate_layouts = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_SLATE_LAYOUT, &slate_count);
    rules.slate_layout_count = slate_count;
    rules.canvas_layouts = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_CANVAS_LAYOUT, &canvas_count);
    rules.canvas_layout_count = canvas_count;
    rules.widget_targets = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_WIDGET_TARGET, &widget_count);
    rules.widget_target_count = widget_count;
    rules.force_shaders = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_FORCE_SHADER, &forced_count);
    rules.force_shader_count = forced_count;
    rules.skip_shaders = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_SKIP_SHADER, &skip_count);
    rules.skip_shader_count = skip_count;
    /* The frame's own target: the back buffer, not the composite.
     *
     * The first run of this got that wrong and the log said so plainly. A draw with Slate's
     * declaration, six indices and a stride of 40, writing into the 2048x1152 back buffer, came
     * back UNKNOWN, because it was being compared against a 1024x576 composite. That draw is the
     * interface at native resolution and is the least ambiguous thing in the frame. */
    rules.back_buffer = bridge.present_target;

    memset(&facts, 0, sizeof(facts));
    facts.struct_size = sizeof(facts);
    facts.input_layout = draw->input_layout;
    facts.vertex_shader = draw->vertex_shader;
    facts.pixel_shader = draw->pixel_shader;
    facts.render_target = draw->render_target;
    facts.target_width = draw->target_width;
    facts.target_height = draw->target_height;
    facts.target_count = draw->target_count;
    facts.depth_bound = draw->depth_bound;
    facts.indexed = draw->indexed;
    facts.element_count = draw->element_count;
    facts.vertex_stride = draw->vertex_stride;
    /* The blend state is shadowed by pointer and its factors are not readable without a device
       call, so the rule sees the over blend AC7's interface uses. Reading the description belongs
       with the divert, which needs it anyway to patch the alpha operations. */
    facts.blend_enabled = 1;
    facts.src_blend = RSF_AC7_BLEND_SRC_ALPHA;
    facts.dest_blend = RSF_AC7_BLEND_INV_SRC_ALPHA;
    for (index = 0; index < draw->input_count && used < RSF_AC7_UI_MAX_INPUTS; ++index) {
        inputs[used].slot = draw->inputs[index].slot;
        inputs[used].texture = draw->inputs[index].texture;
        inputs[used].width = draw->inputs[index].width;
        inputs[used].height = draw->inputs[index].height;
        ++used;
    }
    facts.input_count = used;
    facts.inputs = inputs;

    verdict = rsf_ac7_ui_classify(&rules, &facts);

    /* Confirm a converter's target by watching Slate write into it, which is what the first run
     * showed the descriptor cannot do.
     *
     * Asking for B8G8R8A8, one mip, no array, no multisampling, render target and shader resource,
     * at 1920x1080 matched over a hundred and eighty textures in this game: thirty-two held and a
     * hundred and fifty-one refused for want of room. A shape that common is not an identification,
     * and this is the fifth time in this frame that a rule of the form "the one that matches" has
     * matched something else as well.
     *
     * A Slate draw writing into a target is not a shape, it is the interface being made. It also
     * removes the configured size list from the answer, which the same run showed to be wrong
     * anyway: Slate draws into a 1920x3304 target, presumably something that scrolls, and no list
     * of expected sizes was ever going to contain that.
     *
     * Ordering works out because the converter fills its texture before anything samples it, so by
     * the time the quads are reached their input is already named. */
    if (draw->render_target && draw->render_target != rules.back_buffer &&
        rsf_ui_registry_contains(bridge.ui, RSF_UI_SET_SLATE_LAYOUT, draw->input_layout) &&
        !rsf_ui_registry_contains(bridge.ui, RSF_UI_SET_WIDGET_TARGET, draw->render_target)) {
        if (rsf_ui_registry_add(bridge.ui, RSF_UI_SET_WIDGET_TARGET, draw->render_target) ==
            RSF_UI_OK) {
            publish_ui_candidates();
        }
    }

    if (verdict == RSF_AC7_DRAW_UI_WIDGET_QUAD) {
        /* Both extents, because the gap between them is the whole problem: the interface is
           rasterized at one size and drawn into a target at another. */
        for (index = 0; index < used; ++index) {
            if (inputs[index].width != 0) {
                bridge.ui_widget_extent[0] = inputs[index].width;
                bridge.ui_widget_extent[1] = inputs[index].height;
                break;
            }
        }
        bridge.ui_layer_extent[0] = draw->target_width;
        bridge.ui_layer_extent[1] = draw->target_height;
    }
    return verdict;
}

/* Count what the classifier saw. Counting only: what moves is decided in `ui_verdict`, from the
   same call, so the report and the picture cannot disagree. */
static void on_candidate_draw(void* user, const rsf_frame_tap_target_draw* draw)
{
    rsf_ac7_draw_class verdict;
    (void)user;

    if (!draw || !bridge.ui || !bridge.ui_classify) {
        return;
    }
    ++bridge.ui_candidate_draws;
    verdict = classify_candidate(draw);
    if (verdict < 7) {
        ++bridge.ui_class_counts[verdict];
    }

    if (bridge.ui_traced < 24u && verdict != RSF_AC7_DRAW_SCENE) {
        ++bridge.ui_traced;
        /* The hashes rather than the pointers, because a hash is what a settings file can name and
           a pointer is meaningless the moment the process exits. */
        /* The view format as well as the texture's. They differ whenever the texture is typeless,
           which Unreal's are, and the view is the one that says whether this draw's colour is being
           encoded on the way in. Format 29 is R8G8B8A8_UNORM_SRGB and 28 is plain UNORM. */
        say("  ui draw: class %u, vs 0x%08lx, ps 0x%08lx, layout %p, %s %lu, stride %lu, "
            "target %p %lux%lu texture format %lu view format %lu, depth %lu, targets %lu, "
            "inputs %lu",
            (unsigned)verdict, ui_hash_of(draw->vertex_shader), ui_hash_of(draw->pixel_shader),
            draw->input_layout, draw->indexed ? "indices" : "vertices",
            (unsigned long)draw->element_count, (unsigned long)draw->vertex_stride,
            draw->render_target, (unsigned long)draw->target_width,
            (unsigned long)draw->target_height, (unsigned long)draw->target_format,
            (unsigned long)draw->target_view_format, (unsigned long)draw->depth_bound,
            (unsigned long)draw->target_count, (unsigned long)draw->input_count);
    }
}

static void on_hunt_draw(void* user, const rsf_frame_tap_target_draw* draw)
{
    uint32_t index;
    (void)user;
    if (!draw) {
        return;
    }
    for (index = 0; index < draw->input_count; ++index) {
        if (draw->inputs[index].width != 1920 || draw->inputs[index].height != 1080) {
            continue;
        }
        if (bridge.hunt_logged < RSF_UI_HUNT_LOGGED) {
            ++bridge.hunt_logged;
            say("interface hunt: a 1920x1080 format %lu texture %p in slot %lu is read by a draw "
                "into target %p, %lux%lu format %lu, viewport %lux%lu at %d,%d, %lu elements, "
                "%lu targets bound, depth %s",
                (unsigned long)draw->inputs[index].format, draw->inputs[index].texture,
                (unsigned long)draw->inputs[index].slot, draw->render_target,
                (unsigned long)draw->target_width,
                (unsigned long)draw->target_height, (unsigned long)draw->target_format,
                (unsigned long)draw->viewport_width, (unsigned long)draw->viewport_height,
                (int)draw->viewport_x, (int)draw->viewport_y, (unsigned long)draw->element_count,
                (unsigned long)draw->target_count, draw->depth_bound ? "bound" : "none");
        }
        /* Remember it, but only when this draw is actually a squash.

           The interface's own compositing runs at 1920x1080 and writes 1920x1080, and reads the
           interface while doing it, so it matches the hunt exactly as much as the squash does.
           Promoting those targets is meaningless and it costs plan entries that the real ones need.
           The rule that separates them needs no sizes from elsewhere: a draw that writes a target
           smaller than the texture it is reading is losing detail, and that is the definition of
           the thing being looked for. */
        if (draw->render_target && (draw->target_width < draw->inputs[index].width ||
                                    draw->target_height < draw->inputs[index].height)) {
            uint32_t seen;
            for (seen = 0; seen < bridge.interface_target_count; ++seen) {
                if (bridge.interface_targets[seen] == draw->render_target) {
                    break;
                }
            }
            if (seen == bridge.interface_target_count &&
                bridge.interface_target_count < RSF_REINSERT_MAX_INTERFACE_TARGETS) {
                rsf_resource_retain(draw->render_target);
                bridge.interface_targets[bridge.interface_target_count++] = draw->render_target;
                say("interface hunt: target %p %lux%lu is where the interface is squashed, and is "
                    "now one of %lu to promote",
                    draw->render_target, (unsigned long)draw->target_width,
                    (unsigned long)draw->target_height,
                    (unsigned long)bridge.interface_target_count);
            }
        }
        break;
    }
}

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
/* How many frames of redirecting nothing means the plan no longer describes the frame.

   Generous, because a legitimately quiet stretch exists: a loading screen or a menu can go a while
   without binding the composite. Restaking costs 32 frames of describing the tail again, so being
   slow to react is cheaper than reacting to a pause. */
#define RSF_REINSERT_STALL_FRAMES 240ul

/* Defined below, next to the toggle it shares its work with. Declared here because a stalled plan
   is noticed in the present hook, which runs long before that. */
static int install_reinsert_plan(void);

/* Notice that reinsertion has stopped doing anything, and go and find the tail again.

   The plan matches textures by address. Holding a reference keeps a texture alive, which is not the
   same as keeping it in use: the engine's render target pool is free to hand the composite role to
   a different allocation on a screen change or a resize, and from that moment every substitution
   stops matching. Nothing about that is visible from inside the game, and nothing about it is
   visible in the counters either, because they do not fall, they simply stop rising. That is what
   was happening when reinsertion reported itself on for 18,000 frames having opened 569 gates. */
static void watch_for_stalled_plan(void)
{
    rsf_frame_tap_status tap;

    memset(&tap, 0, sizeof(tap));
    tap.struct_size = sizeof(tap);
    if (rsf_frame_tap_get_status(&tap) != RSF_FRAME_TAP_OK) {
        return;
    }
    if (tap.targets_redirected != bridge.redirects_seen) {
        bridge.redirects_seen = tap.targets_redirected;
        bridge.redirect_stall = 0;
        return;
    }
    if (bridge.tail_restaking || ++bridge.redirect_stall < RSF_REINSERT_STALL_FRAMES) {
        return;
    }

    ++bridge.tail_restakes;
    bridge.redirect_stall = 0;
    bridge.tail_restaking = 1;
    say("reinsert: nothing has been redirected for %lu frames, so the plan no longer names the "
        "textures this frame uses. Looking for the tail again, restake %lu",
        RSF_REINSERT_STALL_FRAMES, bridge.tail_restakes);
    /* Let go of what the plan named before looking, so a stale composite cannot be re-found by
       being the thing already held. The tail walk re-identifies both from the frame itself. */
    rsf_frame_tap_set_plan(NULL);
    rsf_resource_release(bridge.composite);
    bridge.composite = NULL;
    rsf_resource_release(bridge.interface_target);
    bridge.interface_target = NULL;
    /* And the hunt's findings, which go stale the same way and for the same reason. Keeping them
       is worse than having none: the composite is re-identified and promoted while the interface
       targets still name allocations from the previous screen, so the plan looks healthy, the
       counters climb, and the interface is squashed exactly as it was. That is what a title screen
       after an intro looked like. */
    {
        uint32_t index;
        for (index = 0; index < bridge.interface_target_count; ++index) {
            rsf_resource_release(bridge.interface_targets[index]);
            bridge.interface_targets[index] = NULL;
        }
        bridge.interface_target_count = 0;
    }
    bridge.hunt_logged = 0;
    rsf_frame_tap_reset_hunt(RSF_UI_HUNT_DRAWS);
    bridge.tail_frames = 0;
    bridge.tail_draws = 0;
}

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
        /* Looking again is only ever asked for by a stalled plan, so a plan is what it owes.

           Unless the frame it looked at was not the one worth describing. A restake fires 240
           frames after the plan went quiet, which during a screen change lands in whatever is on
           screen at that moment, and the user's run caught one mid video: 38 draws, composite
           found, interface target not found. Installing that leaves the interface at render
           resolution for the whole briefing that follows, which is exactly the symptom reported.

           So an incomplete tail is not accepted. Looking again costs 32 frames and the alternative
           is being wrong until the next stall, which is another 240 frames away at best and never
           at worst, because a partial plan still redirects and so never looks stalled. */
        if (bridge.tail_restaking) {
            if (!bridge.interface_target && bridge.tail_relooks < RSF_TAIL_MAX_RELOOKS) {
                ++bridge.tail_relooks;
                say("frame tail: no interface target in that frame, which is what a video or a "
                    "screen change looks like. Looking again, attempt %lu of %lu",
                    bridge.tail_relooks, RSF_TAIL_MAX_RELOOKS);
                bridge.tail_frames = 0;
                bridge.tail_draws = 0;
                return;
            }
            bridge.tail_restaking = 0;
            bridge.tail_relooks = 0;
            install_reinsert_plan();
        }
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
            /* Against every layer the recombine reads, not one chosen for it. The replay knows
               which texture it drew into, so the match is that texture appearing among this
               frame's inputs, and there is nothing left to guess. */
            uint32_t candidate;
            for (candidate = 0; candidate < bridge.color_selection.composed_layer_count;
                 ++candidate) {
                void* selected = rsf_depth_replay_selected(
                    bridge.depth_replay[i], context, bridge.held_depth,
                    bridge.color_selection.composed_layers[candidate]);
                if (selected != bridge.held_depth) {
                    frame.depth = selected;
                    break;
                }
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
                    "refused %lu; the frame offers %lu layers, first %p, depth %p context %p",
                    slot, detail.layer, detail.source, detail.context, (unsigned long)detail.draws,
                    (unsigned long)detail.refused,
                    (unsigned long)bridge.color_selection.composed_layer_count,
                    bridge.color_selection.composed_layer_count
                        ? bridge.color_selection.composed_layers[0]
                        : NULL,
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

/* Whether the layer encodes on write. Settable because the right answer depends on how the game
   viewed the target these draws came from, and this frame has been wrong about that kind of thing
   before. Defaults to on, which is what the first run's symptoms point at. */
static int ui_layer_srgb = 1;

/* Modules that log take a sink and a user pointer; this bridge's log is a single global. */
static void bridge_layer_log(void* user, const char* message)
{
    (void)user;
    say("%s", message);
}

/* A render target view onto the swap chain's current back buffer, cached.
 *
 * Created once per back buffer rather than per present, and dropped when the texture changes, which
 * is what a resize looks like from here. Holding the view rather than the buffer keeps the
 * reference this needs without the one that would make `ResizeBuffers` fail. */
static void* back_buffer_view(void* swapchain)
{
    static void* cached_for = NULL;
    static void* cached = NULL;
    static int complained = 0;
    void* buffer;

    if (!swapchain || !bridge.device) {
        return NULL;
    }
    /* Asked of the swap chain every present rather than taken from `bridge.back_buffer`.
     *
     * That field belongs to the frame tail walk, which holds it for thirty-two frames and then
     * deliberately lets it go, because a held back buffer reference makes ResizeBuffers fail. The
     * first extraction run read it anyway and got null on every frame, so 17702 draws were diverted
     * out of the scene and none of them were ever composited back. The interface simply vanished,
     * and nothing said why, because the null path was the silent one.
     *
     * The reference from GetBuffer is released as soon as the view exists. The view keeps the
     * surface alive on its own, and it is the view rather than the buffer that has to survive to
     * the draw. */
    buffer = rsf_swapchain_back_buffer(swapchain);
    if (!buffer) {
        if (!complained) {
            complained = 1;
            say("ui extract: the swap chain gave no back buffer, so nothing can be composited");
        }
        return NULL;
    }
    if (buffer != cached_for) {
        rsf_resource_release(cached);
        cached = rsf_create_render_target_view(bridge.device, buffer);
        cached_for = buffer;
        if (!cached && !complained) {
            complained = 1;
            say("ui extract: no view onto the back buffer, so nothing can be composited onto it");
        }
    }
    rsf_resource_release(buffer);
    return cached;
}

/* What the classifier's verdict means to the tap.
 *
 * Only two classes are moved. A Slate draw into the frame's own target is already at output
 * resolution, so moving it gains nothing and risks the one thing in the frame that is currently
 * right. A converter rasterizing its widget must stay where it is or the quads read an empty
 * texture. Modulate is counted and never moved, because it writes colour only and a transparent
 * layer keeps nothing of it. */
static rsf_frame_tap_verdict ui_verdict(void* user, const rsf_frame_tap_target_draw* draw)
{
    rsf_ac7_draw_class verdict;
    (void)user;
    if (!draw) {
        return RSF_FRAME_TAP_LEAVE;
    }
    verdict = classify_candidate(draw);
    if (verdict != RSF_AC7_DRAW_UI_WIDGET_QUAD) {
        return RSF_FRAME_TAP_LEAVE;
    }
    /* Recorded here rather than after the fact: the tap can refuse the divert for reasons this does
       not see, and a layer marked written that nothing wrote would composite a stale frame. The
       count of composites against the count of diverts is what shows the two agreeing. */
    rsf_ui_layer_mark_written(bridge.layer);
    /* The quads are drawn with the base pass translucent blend, whose alpha factors leave a
       transparent layer at zero coverage however much colour lands on it. Measured under DXVK in
       tests/ui_layer.cpp rather than taken from the engine source. */
    return RSF_FRAME_TAP_DIVERT_PATCH_ALPHA;
}

/* Bring up the layer and the compositor, once the device and the presented size are known. */
static int start_extraction(unsigned long width, unsigned long height)
{
    rsf_ui_layer_setup layer;
    rsf_fullscreen_setup pass;

    if (bridge.layer || bridge.ui_extract_failed) {
        return bridge.layer != NULL;
    }
    memset(&layer, 0, sizeof(layer));
    layer.struct_size = sizeof(layer);
    layer.abi_version = RSF_UI_LAYER_ABI_VERSION;
    layer.width = (uint32_t)width;
    layer.height = (uint32_t)height;
    /* Shareable from the start: frame generation opens this on a D3D12 device later and the flag
       cannot be added without recreating the texture. */
    layer.shareable = 1;
    /* Encode on the way in, matching the target the draws were taken from.
     *
     * The first extraction run put the interface on screen at native resolution and it came out
     * dark and desaturated, which is what storing linear values where encoded ones belong looks
     * like. The `ui draw:` trace now prints the view format the game bound, so the next run says
     * whether this is the right answer rather than leaving it as the likeliest one. */
    layer.srgb = ui_layer_srgb;
    layer.log = bridge_layer_log;
    if (rsf_ui_layer_create(bridge.device, &layer, &bridge.layer) != RSF_UI_LAYER_OK) {
        say("ui extract: the layer could not be created; the interface stays in the scene");
        bridge.ui_extract_failed = 1;
        return 0;
    }

    memset(&pass, 0, sizeof(pass));
    pass.struct_size = sizeof(pass);
    pass.abi_version = RSF_FULLSCREEN_PASS_ABI_VERSION;
    pass.log = bridge_layer_log;
    if (rsf_fullscreen_pass_create(bridge.device, &pass, &bridge.composite_pass) !=
        RSF_FULLSCREEN_OK) {
        /* A layer with no compositor is worse than no layer: the interface would be diverted out
           of the frame and never put back. Refuse the whole thing rather than half of it. */
        say("ui extract: the compositor could not be created; the interface stays in the scene");
        rsf_ui_layer_destroy(bridge.layer);
        bridge.layer = NULL;
        bridge.ui_extract_failed = 1;
        return 0;
    }
    say("ui extract: layer and compositor ready at %lux%lu", width, height);
    return 1;
}

/* Put the interface back over the finished frame.
 *
 * Runs inside the present hook, after the game has finished drawing and before anything is shown,
 * which is the one moment the frame exists complete and unseen. Skipped when nothing was diverted,
 * so a frame with no interface on it costs nothing. */
static void composite_ui(void* swapchain)
{
    rsf_ui_layer_status status;
    rsf_fullscreen_draw parameters;
    void* source;
    void* target_view;

    if (!bridge.layer || !bridge.composite_pass) {
        return;
    }
    memset(&status, 0, sizeof(status));
    status.struct_size = sizeof(status);
    if (rsf_ui_layer_get_status(bridge.layer, &status) != RSF_UI_LAYER_OK) {
        return;
    }
    if (status.written_this_frame) {
        source = rsf_ui_layer_source(bridge.layer);
        target_view = back_buffer_view(swapchain);
        if (source && target_view) {
            memset(&parameters, 0, sizeof(parameters));
            parameters.struct_size = sizeof(parameters);
            parameters.mode = RSF_FULLSCREEN_PREMULTIPLIED;
            if (rsf_fullscreen_pass_draw(bridge.composite_pass, bridge.context, target_view, source,
                                         &parameters) == RSF_FULLSCREEN_OK) {
                ++bridge.ui_composites;
            }
        }
    }
    /* The next frame's slot, cleared here rather than after the composite, so a frame that presents
       twice keeps a stale layer rather than losing the interface entirely. */
    rsf_ui_layer_begin_frame(bridge.layer, bridge.context);

    /* And point the divert at it. The layer alternates slots so that a vendor holding the previous
       one is not reading the one being drawn, which means the target the tap writes into changes
       every frame and arming it once would send every frame after the first into the slot being
       composited from. */
    if (bridge.ui_extract) {
        rsf_frame_tap_divert_setup divert;
        memset(&divert, 0, sizeof(divert));
        divert.struct_size = sizeof(divert);
        divert.layer_target = rsf_ui_layer_target(bridge.layer);
        divert.layer_width = status.width;
        divert.layer_height = status.height;
        divert.verdict = ui_verdict;
        rsf_frame_tap_set_divert(&divert);
    }
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
    stats->jitter_on = bridge.actions.jitter_open ? (uint32_t)bridge.actions.jitter_open() : 0u;
    stats->jitter_available =
        bridge.actions.jitter_available ? (uint32_t)bridge.actions.jitter_available() : 0u;
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
    if (intent.jitter_changed) {
        if (bridge.actions.set_jitter) {
            bridge.actions.set_jitter(intent.jitter);
        } else {
            say("overlay: this build has no jitter control registered");
        }
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

    /* Before anything reads it. The classifier compares against this on every candidate draw of the
       next frame, and a run where it is null classifies every Slate draw into the frame's own
       target as unknown, which is what happened the first time. */
    {
        void* buffer = rsf_swapchain_back_buffer(swapchain);
        if (buffer) {
            bridge.present_target = buffer;
            /* The pointer outlives the reference deliberately: the swap chain owns the surface and
               this is only ever compared against a bound target, never dereferenced. Holding the
               reference is what makes ResizeBuffers fail. */
            rsf_resource_release(buffer);
        }
    }

    if (bridge.frames_described < 4 && bridge.pass_in_frame > 0) {
        say("frame ended after %lu qualifying passes", bridge.pass_in_frame);
        ++bridge.frames_described;
    }
    bridge.pass_in_frame = 0;

    /* The interface goes back on before anything else looks at the frame, and independently of
       whether a reconstruction is running: extraction is about where the interface is drawn, not
       about super resolution, and it has to be judgeable on its own. */
    if (bridge.ui_extract) {
        composite_ui(swapchain);
    }

    if (bridge.started) {
        watch_tail(swapchain);
        if (bridge.reinsert_on) {
            /* The evaluate already happened, at the gate, where it has to happen for the result to
               reach the game's own tonemap. All that is left is to let go of the frame's inputs and
               to close the gates so the next frame opens them again. */
            ++bridge.reinsert_frames;
            watch_for_stalled_plan();
            release_held();
            rsf_frame_tap_end_frame();
        } else {
            /* The frame is finished here, which is the whole reason the evaluate waits for it when
               the result is only being drawn over the top. */
            evaluate_held(bridge.context);
        }
    }

    /* Before the per-frame state is cleared. The count is only ever nonzero once the frame tap is
       installed, which happens when the backend starts, so a briefing reached without starting one
       reports zero and the layer keeps its ordinary scale. That is a real limit and not a
       deliberate choice: nothing observes draws before the tap exists. */
    bridge.translucent_indices_last = bridge.translucent_indices;
    bridge.translucent_draws_last = bridge.translucent_draws;
    if (bridge.actions.translucent_geometry) {
        bridge.actions.translucent_geometry(bridge.translucent_indices);
    }
    bridge.translucent_indices = 0;
    bridge.translucent_draws = 0;

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

int rsf_bridge_extract_ui(unsigned long width, unsigned long height)
{
    rsf_frame_tap_divert_setup divert;

    if (!bridge.ui || !bridge.device) {
        say("ui extract: nothing is being classified, so there is nothing to extract");
        return 0;
    }
    if (!start_extraction(width, height)) {
        return 0;
    }
    memset(&divert, 0, sizeof(divert));
    divert.struct_size = sizeof(divert);
    divert.layer_target = rsf_ui_layer_target(bridge.layer);
    divert.layer_width = (uint32_t)width;
    divert.layer_height = (uint32_t)height;
    divert.verdict = ui_verdict;
    if (!divert.layer_target) {
        /* The first frame has not begun, so there is no current slot yet. Begin one here: the layer
           is armed before the frame it belongs to rather than after. */
        rsf_ui_layer_begin_frame(bridge.layer, bridge.context);
        divert.layer_target = rsf_ui_layer_target(bridge.layer);
    }
    if (rsf_frame_tap_set_divert(&divert) != RSF_FRAME_TAP_OK) {
        say("ui extract: the frame tap refused the divert");
        return 0;
    }
    bridge.ui_extract = 1;
    say("ui extract: on. Interface draws now go to a %lux%lu layer and are composited at present",
        width, height);
    return 1;
}

/* Parse a comma or space separated list of hex hashes, as a settings file writes them.
 *
 * Tolerant on purpose: these are typed by hand off a log line, so an 0x prefix is optional and any
 * punctuation between numbers separates them. Anything unparseable is skipped and counted, because
 * silently ignoring a shader somebody meant to name is how an escape hatch stops being one. */
static unsigned int parse_hash_list(const char* text, unsigned long* out, unsigned int capacity,
                                    unsigned int* rejected)
{
    unsigned int count = 0;
    const char* cursor = text;

    if (!text) {
        return 0;
    }
    while (*cursor && count < capacity) {
        char* end = NULL;
        unsigned long value;
        while (*cursor && !isxdigit((unsigned char)*cursor)) {
            ++cursor;
        }
        if (!*cursor) {
            break;
        }
        value = strtoul(cursor, &end, 16);
        if (end == cursor) {
            ++cursor;
            if (rejected) {
                ++*rejected;
            }
            continue;
        }
        out[count++] = value;
        cursor = end;
    }
    return count;
}

void rsf_bridge_set_ui_encoding(int srgb)
{
    ui_layer_srgb = srgb != 0;
}

void rsf_bridge_name_shaders(const char* forced, const char* skipped)
{
    unsigned int rejected = 0;
    ui_forced_count = parse_hash_list(forced, ui_forced_hashes, RSF_UI_MAX_NAMED, &rejected);
    ui_skipped_count = parse_hash_list(skipped, ui_skipped_hashes, RSF_UI_MAX_NAMED, &rejected);
    if (ui_forced_count || ui_skipped_count || rejected) {
        say("ui: %u shaders named as interface, %u as not, %u entries unreadable",
            ui_forced_count, ui_skipped_count, rejected);
    }
}

int rsf_bridge_identify_ui(void)
{
    if (!bridge.ui) {
        bridge.ui = rsf_ui_registry_create(RSF_UI_IDENTIFY_ABI_VERSION);
        if (!bridge.ui) {
            return 0;
        }
    }
    bridge.ui_classify = 1;
    return 1;
}

rsf_observer_layout_fn rsf_bridge_layout_hook(void) { return on_layout_created; }
rsf_observer_shader_fn rsf_bridge_shader_hook(void) { return on_shader_created; }
rsf_observer_texture_fn rsf_bridge_texture_hook(void) { return on_texture_created; }

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
    rsf_dlss_pipeline_status pipeline;
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

    if (!install_reinsert_plan()) {
        return;
    }
    bridge.reinsert_on = 1;
    /* Deliberately not "reinsertion works". The substitutions are in place and the game will draw
       its own tail over the reconstruction; whether the result is right is a thing to look at. */
    say("reinsert: on. The scene is reconstructed before the game's tonemap, and the grade and the "
        "interface are the game's own. Press F6 again to stop");
}

/* Build the substitutions for the tail as it currently stands and hand them to the tap.

   Separate from the toggle because it is also what a stalled plan needs. Nothing here decides
   whether reinsertion should be on; it only makes the plan describe the frame that is actually
   being drawn now. */
static int install_reinsert_plan(void)
{
    rsf_reinsert_frame_tail tail;
    rsf_frame_tap_plan plan;
    rsf_reinsert_result prepared;
    uint32_t index;
    void* reconstruction = rsf_dlss_pipeline_output_texture();

    if (!bridge.reinsert || !bridge.composite || !bridge.scene_color || !reconstruction ||
        bridge.held_width == 0 || bridge.held_height == 0) {
        say("reinsert: the plan cannot be built, composite %s, scene colour %s, reconstruction %s, "
            "render size %lux%lu",
            bridge.composite ? "found" : "missing", bridge.scene_color ? "found" : "missing",
            reconstruction ? "found" : "missing", bridge.held_width, bridge.held_height);
        return 0;
    }

    memset(&tail, 0, sizeof(tail));
    tail.struct_size = sizeof(tail);
    tail.composite = bridge.composite;
    /* The hunt's findings first, because they are an observation rather than a rule: each one is a
       target a draw reading the 1920x1080 interface actually wrote into. The tail's own guess is
       taken only when the hunt has found nothing yet, and only if it is not already in the set. */
    for (index = 0; index < bridge.interface_target_count; ++index) {
        tail.interface_targets[tail.interface_target_count++] = bridge.interface_targets[index];
    }
    if (bridge.interface_target && tail.interface_target_count == 0) {
        tail.interface_targets[tail.interface_target_count++] = bridge.interface_target;
    }
    tail.scene_color = bridge.scene_color;
    tail.reconstruction = reconstruction;
    tail.render_width = (uint32_t)bridge.held_width;
    tail.render_height = (uint32_t)bridge.held_height;
    prepared = rsf_reinsert_prepare(bridge.reinsert, &tail);
    if (prepared == RSF_REINSERT_ERROR_NOT_SCALED) {
        say("reinsert: press F9 to put the render scale back first, there is nothing to upscale");
        return 0;
    }
    if (prepared != RSF_REINSERT_OK) {
        say("reinsert: the replacements could not be prepared, result %d", (int)prepared);
        return 0;
    }

    memset(&plan, 0, sizeof(plan));
    plan.struct_size = sizeof(plan);
    if (rsf_reinsert_fill_plan(bridge.reinsert, &plan) != RSF_REINSERT_OK) {
        say("reinsert: the plan could not be built");
        return 0;
    }
    plan.on_gate = on_gate;
    /* A promoted target and the game's render resolution depth are a pair D3D11 rejects, so
       whatever the game draws with depth into the composite is lost unless something gives. The
       three answers are all wrong in different ways and the setting exists so all three can be
       compared in one run rather than one per build. See `depth_policy`. */
    plan.depth_policy = bridge.actions.reinsert_depth_policy
                            ? (uint32_t)bridge.actions.reinsert_depth_policy()
                            : RSF_FRAME_TAP_DEPTH_DROP;

    if (rsf_frame_tap_set_plan(&plan) != RSF_FRAME_TAP_OK) {
        say("reinsert: the frame tap refused the plan");
        return 0;
    }
    /* The stall detector measures from here, so a fresh plan is never mistaken for a stalled one
       just because the previous plan's redirects are still the last thing counted. */
    bridge.redirect_stall = 0;
    return 1;
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
    /* Find whatever composites the interface, by the shape it reads rather than by a rule about
       formats. `UWidgetToTextureConverter_Setup` is handed a hardcoded 1920x1080 at 0x1406243e0,
       so a draw reading a 1920x1080 R8G8B8A8 texture is compositing the interface, and the target
       it writes into is the one that has to be at output resolution. Every format rule tried so far
       has picked the wrong surface at least once, which is why this starts from a size the binary
       states rather than from a guess. 28 is DXGI_FORMAT_R8G8B8A8_UNORM. */
    bridge.output_width = output_width;
    bridge.output_height = output_height;
    tap.hunt_width = 1920;
    tap.hunt_height = 1080;
    /* Any format. The first attempt asked for R8G8B8A8_UNORM, which is 28, and found nothing across
       a whole session: the shadow carries the texture's own format and AC7's colour targets are
       created typeless, 27, for the view to reinterpret. Naming a format here is how the hunt
       reproduced the mistake it exists to avoid. The size comes from the binary; the format is
       reported rather than assumed. */
    tap.hunt_format = 0;
    tap.hunt_budget = RSF_UI_HUNT_DRAWS;
    tap.on_hunt_draw = on_hunt_draw;
    tap.on_candidate_draw = on_candidate_draw;
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
    /* After the tap is installed, not before: the jitter is only worth having once there is
       something reading the frames it belongs to. */
    if (bridge.actions.set_jitter) {
        bridge.actions.set_jitter(1);
    }
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
    if (bridge.ui) {
        rsf_ui_registry_counters counters;
        uint32_t slate = 0, canvas = 0, widget = 0;
        int moved = 0;
        unsigned int index;
        memset(&counters, 0, sizeof(counters));
        counters.struct_size = sizeof(counters);
        rsf_ui_registry_get_counters(bridge.ui, &counters);
        rsf_ui_registry_view(bridge.ui, RSF_UI_SET_SLATE_LAYOUT, &slate);
        rsf_ui_registry_view(bridge.ui, RSF_UI_SET_CANVAS_LAYOUT, &canvas);
        rsf_ui_registry_view(bridge.ui, RSF_UI_SET_WIDGET_TARGET, &widget);
        say("ui: named %lu slate layouts, %lu canvas layouts, %lu widget targets; %lu addresses "
            "forgotten on reuse, %lu adds refused full",
            (unsigned long)slate, (unsigned long)canvas, (unsigned long)widget,
            (unsigned long)counters.forgotten_on_reuse,
            (unsigned long)(counters.refused_full[RSF_UI_SET_SLATE_LAYOUT] +
                            counters.refused_full[RSF_UI_SET_CANVAS_LAYOUT] +
                            counters.refused_full[RSF_UI_SET_WIDGET_TARGET]));
        /* Quiet when nothing has changed, like the rest of this report: a screen that is holding
           still should stop writing rather than filling the log with the same line. */
        for (index = 0; index < 7; ++index) {
            moved = moved || bridge.ui_class_counts[index] != bridge.ui_reported_counts[index];
            bridge.ui_reported_counts[index] = bridge.ui_class_counts[index];
        }
        if (moved) {
            /* "scene" is the class named RSF_AC7_DRAW_SCENE: a draw that passed the prefilter and
               turned out to be nothing of ours. The first run labelled this column
               "canvas-into-frame", which is not a class at all, and made forty thousand scene
               draws read as interface. */
            say("ui: %lu candidate draws: slate %lu, widget quad %lu, converter raster %lu, "
                "modulate %lu, scene %lu, unknown %lu, skipped %lu",
                bridge.ui_candidate_draws, bridge.ui_class_counts[RSF_AC7_DRAW_UI_SLATE],
                bridge.ui_class_counts[RSF_AC7_DRAW_UI_WIDGET_QUAD],
                bridge.ui_class_counts[RSF_AC7_DRAW_WIDGET_RASTER],
                bridge.ui_class_counts[RSF_AC7_DRAW_UI_MODULATE],
                bridge.ui_class_counts[RSF_AC7_DRAW_SCENE],
                bridge.ui_class_counts[RSF_AC7_DRAW_UNKNOWN],
                bridge.ui_class_counts[RSF_AC7_DRAW_SKIP]);
            if (bridge.ui_extract) {
                rsf_frame_tap_status divert_status;
                rsf_ui_layer_status layer_status;
                memset(&divert_status, 0, sizeof(divert_status));
                divert_status.struct_size = sizeof(divert_status);
                rsf_frame_tap_get_status(&divert_status);
                memset(&layer_status, 0, sizeof(layer_status));
                layer_status.struct_size = sizeof(layer_status);
                rsf_ui_layer_get_status(bridge.layer, &layer_status);
                say("ui extract: %lu draws diverted, %lu blends patched, %lu refused (last reason "
                    "%lu), %llu frames wrote the layer, %lu composited",
                    (unsigned long)divert_status.draws_diverted,
                    (unsigned long)divert_status.blend_states_patched,
                    (unsigned long)divert_status.divert_refused,
                    (unsigned long)divert_status.divert_last_refusal,
                    (unsigned long long)layer_status.frames_written, bridge.ui_composites);
                /* The one combination that means the interface has been taken out of the frame and
                   not put back, which is exactly what the first run did and what nothing said at
                   the time. Worth its own sentence rather than being left as two numbers a reader
                   has to compare. */
                if (layer_status.frames_written > 0 && bridge.ui_composites == 0) {
                    say("ui extract: the interface is being diverted and never composited, so it "
                        "is missing from the picture entirely. The layer has no target.");
                }
            }
            if (bridge.ui_widget_extent[0]) {
                say("ui: the interface is rasterized at %lux%lu and drawn into %lux%lu, which is "
                    "the gap promotion cannot close",
                    bridge.ui_widget_extent[0], bridge.ui_widget_extent[1],
                    bridge.ui_layer_extent[0], bridge.ui_layer_extent[1]);
            }
        }
    }
    say("translucent layer: last completed frame drew %lu indices in %lu draws into it",
        bridge.translucent_indices_last, bridge.translucent_draws_last);
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
        /* The number that says whether geometry is being dropped. A promoted target bound with the
           game's own depth is an invalid pair, so the pass draws nothing, and flat interface draws
           carry no depth and are untouched. That is exactly the shape of an interface that looks
           right over a scene that is missing. */
        say("reinsert: %lu depth mismatches, policy %lu (0 drop, 1 keep, 2 refuse), last pair "
            "target %lux%lu against depth %lux%lu format %lu",
            (unsigned long)tap.depth_mismatches,
            bridge.actions.reinsert_depth_policy
                ? (unsigned long)bridge.actions.reinsert_depth_policy()
                : 0ul,
            (unsigned long)tap.depth_mismatch_target_width,
            (unsigned long)tap.depth_mismatch_target_height,
            (unsigned long)tap.depth_mismatch_depth_width,
            (unsigned long)tap.depth_mismatch_depth_height,
            (unsigned long)tap.depth_mismatch_depth_format);
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
