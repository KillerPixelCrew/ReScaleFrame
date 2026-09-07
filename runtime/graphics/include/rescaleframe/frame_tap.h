/* SPDX-License-Identifier: GPL-3.0-only */
/* Find the frame's reconstruction inputs while the game is rendering them.

   A super resolution backend needs scene colour, motion, depth and an exposure value from the same
   frame, at render resolution, before tonemapping. `docs/research/ac7-frame-capture.md` shows that
   Ace Combat 7 already binds all of them together: one pass takes scene colour, a history target,
   the velocity target, depth and a 1x1 exposure target in a single call. That combination occurs
   once in the frame, so the binding itself is the identification.

   Earlier research named that pass after the Unreal pass whose inputs match. That was an inference
   and it is not repeated here. The game offers only FXAA and none as anti-aliasing modes, this
   project's jitter patch deliberately does not turn a temporal mode on, and nothing at runtime
   labels a pass. What is established is the input set, so that is what this looks for.

   Identification comes from the bound resources alone: each view's resource is queried as a
   texture and classified through `resource_roles.h`. Nothing is remembered from creation time, so
   this module needs neither the observer nor agreement with it, and it works on textures allocated
   before it was installed.

   The callback runs synchronously inside the hook, on the game's render thread. That is the only
   moment the resources hold this frame's contents; anything deferred to present would read the
   frame after it finished.

   The same shadowed bindings answer a second question, which is where the reconstruction goes back
   in. `docs/research/ac7-frame-capture.md` establishes that at a reduced render scale the frame
   ends with one draw that reads a single render resolution composite and writes the output
   resolution back buffer, and that the interface is composited into that composite before it. So
   the scene has to be replaced before those composite draws, not at the last one. Which draws
   write that target and what each of them reads is not in any capture in this repository: the
   exported action list records render target bindings and not shader resource bindings, which is
   stated as a limitation there twice. It is in the shadow this module already keeps.

   Hence `rsf_frame_tap_watch_target`. Name a render target and the tap reports the next few draws
   into it with their pixel shader inputs, sizes and viewport. Point it at the back buffer and the
   answer names the composite; point it at the composite and the answer names the draw that reads
   scene colour, which is the one to intervene at.

   And then the intervention itself, because this module owns the hooks the substitution has to
   happen in. `rsf_frame_tap_set_plan` names textures to swap out: a shader resource view onto one
   of them is bound as something else, a render target view onto one is bound as something else,
   and viewports and scissor rectangles are scaled while a substituted target is bound. That is
   enough to make the game draw its own tail at output resolution over a reconstructed scene, and
   `scene_reinsert.h` is what decides which textures those are and creates the replacements.

   The watch is an observer and the plan is not, so the honest split is per call rather than per
   module: a binding named by the plan is altered before it is forwarded, and everything else is
   forwarded first and looked at afterwards. The shadow always records what the game asked for
   rather than what was bound in its place, because everything else here is a description of the
   game's frame and would stop being one otherwise. */

#ifndef RSF_FRAME_TAP_H
#define RSF_FRAME_TAP_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_FRAME_TAP_ABI_VERSION 7u

/* Render targets watched at once. Two, because the question this answers needs exactly two: the
   swap chain's back buffer, and whichever target the draw into it reads. */
#define RSF_FRAME_TAP_WATCH_SLOTS 2u

/* Pixel shader resources reported per watched draw. Sixteen covers every post process pass in this
   game's frame; a pass binding more is reported truncated, with `input_count` saying so, rather
   than not reported at all. */
#define RSF_FRAME_TAP_MAX_INPUTS 16u

/* Textures a plan may substitute at once. Three, plus room: the composite, the interface's target
   and the scene colour is the whole of the tail this was written for. */
/* Eight, because four was exactly the composite, scene colour and one interface target with one to
   spare, and the interface turned out to be composited into more than one surface. A plan that
   describes the frame correctly and is then refused for being one entry too long is a silent
   failure: reinsertion reports itself on and nothing is substituted. */
#define RSF_FRAME_TAP_MAX_SUBSTITUTIONS 8u

typedef int32_t rsf_frame_tap_result;
#define RSF_FRAME_TAP_OK ((rsf_frame_tap_result)0)
#define RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT ((rsf_frame_tap_result)-1)
#define RSF_FRAME_TAP_ERROR_ABI_MISMATCH ((rsf_frame_tap_result)-2)
#define RSF_FRAME_TAP_ERROR_ALREADY_INSTALLED ((rsf_frame_tap_result)-3)
#define RSF_FRAME_TAP_ERROR_NOT_INSTALLED ((rsf_frame_tap_result)-4)
#define RSF_FRAME_TAP_ERROR_PATCH_FAILED ((rsf_frame_tap_result)-5)

/* Same shape and same reason as the other sinks here: this runs inside a game's render thread,
   where a returned code often never arrives. Called before a step rather than after it. */
typedef void (*rsf_frame_tap_log_fn)(void* user, const char* message);

/* One recognised binding of the reconstruction input set.

   Every pointer is borrowed for the duration of the callback and must not be retained past it. The
   tap holds a reference on each while calling, and drops it on return; keeping one afterwards means
   holding a reference of your own, which changes the lifetime of a resource the game pools and
   reuses. Copy what you need instead. */
typedef struct rsf_frame_tap_pass {
    uint32_t struct_size;
    /* The installed `ID3D11DeviceContext*` that made the call. */
    void* context;
    /* `ID3D11Texture2D*`, taken to be the view bound at slot 0. Unreal's post process inputs put
       the primary input there, and several full resolution targets in this frame share scene
       colour's descriptor, so nothing in the binding distinguishes it. This assumption cannot be
       checked without the running game. */
    void* scene_color;
    /* `ID3D11Texture2D*`, the second target sharing scene colour's shape. Null when only one was
       bound, which is what the first frame after a cut or a resolution change looks like. */
    void* history;
    /* `ID3D11Texture2D*`, the engine's velocity target, still in the engine's own encoding. */
    void* motion;
    /* `ID3D11Texture2D*`. */
    void* depth;
    /* `ID3D11Texture2D*`, the 1x1 eye adaptation target. A 1x1 texture is part of what qualifies
       the set, so this is non-null in practice. The field stays nullable because the qualifying
       rule is a property of this game's frame rather than of the ABI. */
    void* exposure;
    /* `ID3D11Buffer*`, the most recent buffer of `view_constant_bytes` bound to the pixel stage.
       Null until one has been seen. It is the buffer bound around this pass rather than a buffer
       proven to belong to it: constant buffer bindings are not part of the signature that
       identifies the set, and which buffer holds view data is the caller's question. */
    void* view_constants;
    /* Render resolution, read from the motion target, which follows render resolution. */
    uint32_t render_width;
    uint32_t render_height;
    /* Counts qualifying passes, not presented frames. Nothing here observes presentation, and the
       captures show more than one qualifying binding in a frame, so treat this as a sequence
       number for correlating callbacks rather than as a frame count. */
    uint32_t frame_index;
    /* The DXGI format of the colour that was picked. The set is recognised more than once in a
       frame and the passes differ in what their colour holds, so this is how a caller tells them
       apart without querying the texture again. */
    uint32_t scene_color_format;
} rsf_frame_tap_pass;

/* Called on the render thread, inside the hook, with the game's own bindings still in place.
   Anything done here is on the frame's critical path. */
typedef void (*rsf_frame_tap_fn)(void* user, const rsf_frame_tap_pass* pass);

/* One pixel shader resource a reported draw had bound. */
typedef struct rsf_frame_tap_input {
    /* The stage slot, kept rather than implied by position, because a slot holding something that
       is not a 2D texture is still reported and the numbering has to survive it. */
    uint32_t slot;
    /* `ID3D11Texture2D*`, borrowed for the duration of the callback like everything else here.
       Null when the bound view's resource is a buffer or a volume texture. */
    void* texture;
    uint32_t width;
    uint32_t height;
    uint32_t format;
} rsf_frame_tap_input;

/* One draw into a render target named by `rsf_frame_tap_watch_target`.

   This is a description of the game's own draw, taken after it has been forwarded. Nothing about
   the draw is altered. */
typedef struct rsf_frame_tap_target_draw {
    uint32_t struct_size;
    /* The `ID3D11DeviceContext*` that made the call. */
    void* context;
    /* Which watch slot matched. */
    uint32_t watch_index;
    /* `ID3D11Texture2D*` behind the render target view at output slot 0. */
    void* render_target;
    uint32_t target_width;
    uint32_t target_height;
    uint32_t target_format;
    /* Viewport 0 as the draw saw it. Unreal renders into a sub-rectangle of a pooled target often
       enough that the target's own extent does not say what the draw covers, and at a reduced
       render scale the difference between the two is the whole question. Zero when the context
       reported no viewport. */
    uint32_t viewport_width;
    uint32_t viewport_height;
    /* Ordinal among the draws into this target since it was last bound, starting at zero. This is
       what separates "the tonemap, first draw into the composite" from "the eleventh interface
       element drawn on top of it". */
    uint32_t draw_index;
    /* Non-zero for `DrawIndexed`. Captured fullscreen passes include indexed and unindexed triangles;
       callers must consider both this flag and the element count. */
    uint32_t indexed;
    uint32_t element_count;
    /* Occupied pixel shader slots, and the first `RSF_FRAME_TAP_MAX_INPUTS` of them. `input_count`
       is what was reported, not what was bound, when the two differ. */
    uint32_t input_count;
    const rsf_frame_tap_input* inputs;
    /* Additional draw facts for consumers of an input watch. */
    uint32_t depth_bound;
    uint32_t target_count;
    uint32_t target_samples;
    float viewport_x;
    float viewport_y;
    uint32_t inputs_truncated;
    /* Appended in ABI 7. What the pipeline had bound, by pointer.

       These are the identity of a draw, and they are pointers rather than descriptions because
       that is what makes the question cheap: what each object is was settled once when the game
       created it, and at the draw the answer is a comparison against a set. Compare them, never
       dereference them. A pointer is meaningful only while it is bound, and D3D11 reuses an
       address as soon as an object is released, so a set built from these must evict on reuse.

       `vertex_stride` is slot zero's, which is 40 for Slate's geometry and 44 for the canvas: a
       check on the declaration rather than a substitute for it. */
    void* pixel_shader;
    void* vertex_shader;
    void* input_layout;
    void* blend_state;
    void* depth_stencil_state;
    uint32_t vertex_stride;
    uint32_t topology;
} rsf_frame_tap_target_draw;

/* Called on the render thread, immediately after the game's own draw has been forwarded. */
typedef void (*rsf_frame_tap_target_fn)(void* user, const rsf_frame_tap_target_draw* draw);

/* The pipeline objects worth a second look, so the tap can reject the rest of the frame inline.

   A frame is tens of thousands of draws and a handful of them are the interface. Asking a callback
   about each one would put a call on the game's hottest path to be told "no" almost every time, so
   the test lives here and only a draw that passes it is reported. The test is pointer comparisons
   against these sets and nothing else: no device calls, no descriptors, no allocation.

   The arrays are copied, so the caller may rebuild its own storage freely afterwards. Membership is
   by address, which is only meaningful while an object is alive, so whoever fills these has to drop
   an address the moment the game releases it. See `ui_identify.h`, which exists for that. */
typedef struct rsf_frame_tap_candidates {
    uint32_t struct_size;
    /* Input layouts that name an interface producer. */
    void* const* layouts;
    uint32_t layout_count;
    /* `ID3D11Texture2D*` a converter rasterizes a widget into. A draw reading one in a low pixel
       slot is a candidate: it may be drawing the interface, or merely have it left bound. */
    void* const* widget_targets;
    uint32_t widget_target_count;
    /* Shaders a setting named, in either direction. Always candidates, because the point of naming
       one is to reach a draw the rules got wrong. */
    void* const* shaders;
    uint32_t shader_count;
} rsf_frame_tap_candidates;

/* How many pixel shader slots the prefilter examines for a widget target. The world space quads
   read theirs in slot 0; a few textures deeper is cheap and covers a shader that binds a sampler
   ahead of its texture. Beyond that the odds of a stale binding outweigh the odds of a real read. */
#define RSF_FRAME_TAP_CANDIDATE_SLOTS 4u

/* Replace the candidate sets. Empty or null sets switch the prefilter off, which is the default and
   costs a single load per draw. */
rsf_frame_tap_result rsf_frame_tap_set_candidates(const rsf_frame_tap_candidates* candidates);

/* What to do with a candidate draw, decided before it is forwarded.

   The tap cannot decide this. Which draws are the interface is a game fact and lives in
   `games/<id>`; what the tap owns is the mechanism, so the verdict comes back through this and the
   retargeting is done here. */
typedef uint32_t rsf_frame_tap_verdict;
/* Leave the draw alone. Everything that is not the interface, which is almost everything. */
#define RSF_FRAME_TAP_LEAVE ((rsf_frame_tap_verdict)0)
/* Send it to the layer instead of to the target the game bound. */
#define RSF_FRAME_TAP_DIVERT ((rsf_frame_tap_verdict)1)
/* Divert, and patch the blend's alpha operations to `One / InvSrcAlpha` first.
 *
 * Needed for anything drawn with Unreal's base pass translucent blend, whose alpha factors are
 * `Zero / InvSrcAlpha`: on a layer cleared to zero the alpha stays at zero however much colour
 * lands, so the layer composites to nothing while looking, in a debug view, exactly like a divert
 * that never happened. Slate's own blend already accumulates and must be left alone. The colour
 * factors are never touched. */
#define RSF_FRAME_TAP_DIVERT_PATCH_ALPHA ((rsf_frame_tap_verdict)2)

/* Called before the game's draw is forwarded, for every draw that passes the candidate prefilter.
   Runs on the render thread inside the hook; must not call into D3D11. */
typedef rsf_frame_tap_verdict (*rsf_frame_tap_verdict_fn)(void* user,
                                                          const rsf_frame_tap_target_draw* draw);

/* Why a divert did not happen, most recent first in the status. Counted rather than logged: this
   runs per draw and a refusal is normal, but a refusal that becomes common is a rule going wrong
   and the counts are how that is noticed. */
#define RSF_FRAME_TAP_REFUSED_NO_LAYER 1u
/* Several render targets. Moving slot zero changes what the others mean. */
#define RSF_FRAME_TAP_REFUSED_MULTIPLE_TARGETS 2u
/* Unordered access views bound, which are written wherever the draw decides and cannot follow. */
#define RSF_FRAME_TAP_REFUSED_UAV 3u
/* The draw already writes the layer, so there is nothing to move. */
#define RSF_FRAME_TAP_REFUSED_ALREADY_LAYER 4u
/* The blend could not be patched, and diverting without the patch would produce a layer with no
   coverage. Refusing leaves the interface in the scene, which is worse than sharp and better than
   absent. */
#define RSF_FRAME_TAP_REFUSED_BLEND 5u

typedef struct rsf_frame_tap_divert_setup {
    uint32_t struct_size;
    /* `ID3D11RenderTargetView*` of the layer to divert into, or null to stop diverting. Borrowed:
       the caller keeps it alive for as long as it is set. */
    void* layer_target;
    /* The layer's extent, so a draw into a render-resolution target can have its viewport scaled up
       to cover the same fraction of the layer. */
    uint32_t layer_width;
    uint32_t layer_height;
    /* Asked for each candidate before it is forwarded. Null stops diverting. */
    rsf_frame_tap_verdict_fn verdict;
    void* verdict_user;
} rsf_frame_tap_divert_setup;

/* Arm or disarm diverting. Null, or a null layer or verdict, disarms.

   Nothing is diverted until this is called, so the classification milestone and the divert are the
   same code with this switched off. */
rsf_frame_tap_result rsf_frame_tap_set_divert(const rsf_frame_tap_divert_setup* setup);

/* One texture the plan replaces.

   Every pointer here is a D3D11 interface the caller owns and keeps alive for as long as the plan
   is set. Nothing is retained: this module compares `texture` by address and never dereferences it,
   and hands the views straight to the runtime. */
typedef struct rsf_frame_tap_substitution {
    /* `ID3D11Texture2D*` to look for behind a view the game is binding. */
    void* texture;
    /* `ID3D11ShaderResourceView*` bound in place of any view onto `texture`. Null leaves shader
       resource bindings of it alone. */
    void* shader_view;
    /* `ID3D11RenderTargetView*` bound in place of any view onto `texture`, in both the output
       merger and `ClearRenderTargetView`. Null leaves render target bindings of it alone. A
       non-null one is also what makes viewports scale while it is bound. */
    void* render_view;
    /* `ID3D11Texture2D*`. Null means this substitution applies from the start of the frame.
       Otherwise it applies only after that texture has been bound as a render target in the current
       frame, and `rsf_frame_tap_end_frame` closes it again.

       This exists for scene colour. The scene passes read scene colour while they are still writing
       it, and substituting there would hand a pass a reconstruction of the frame it has not
       finished drawing. Gating on the composite being bound means the substitution begins where the
       post chain does. */
    void* after_target;
} rsf_frame_tap_substitution;

/* Called the first time in a frame that a substitution's `after_target` is bound as a render
   target, before that binding is forwarded.

   It is the one moment where the scene is finished and nothing downstream has read it yet, which
   is where a reconstruction has to run. Whatever it does to the device context it must put back:
   the game is midway through its frame and will not rebind what it believes is still there.
   `d3d11_state.h` exists for that. */
typedef void (*rsf_frame_tap_gate_fn)(void* user, void* context, void* texture);

typedef struct rsf_frame_tap_plan {
    uint32_t struct_size;
    uint32_t count;
    rsf_frame_tap_substitution items[RSF_FRAME_TAP_MAX_SUBSTITUTIONS];
    /* Applied to viewports and scissor rectangles while a substituted render target is bound. The
       game asks for the resolution it believes it is drawing at, which is the render resolution,
       and the substituted target is at output resolution. One means no scaling. */
    float viewport_scale_x;
    float viewport_scale_y;
    rsf_frame_tap_gate_fn on_gate;
    void* on_gate_user;
    /* What to do when a substituted target is bound alongside a depth stencil that is not the same
       size as it.

       A promoted target is at output resolution while the game's depth is still at render
       resolution, and D3D11 refuses that pair, so every draw in such a pass is dropped. Flat
       interface draws bind no depth and are unaffected, which is how this loses scene geometry and
       leaves the interface looking correct.

       There is no right answer available here, only three wrong ones with different costs, so it is
       a choice rather than a rule:

         DROP    unbind the depth and keep the substitution. The pass draws, without its depth test
                 or depth writes. Content survives, occlusion within the pass does not.
         KEEP    forward both, which is what this did before the mismatch was understood. The pair
                 is invalid and the pass draws nothing.
         REFUSE  leave the target alone for that binding. The pass draws correctly into the game's
                 own render resolution target, which nothing downstream reads once the promoted one
                 is in the frame, so its content is lost anyway.

       The honest fix is a depth of the right size with the right contents, which needs a resource
       this module does not own and a rescale pass that does not exist yet. Until then DROP is the
       default because it is the only one of the three that keeps the pixels. */
    uint32_t depth_policy;
} rsf_frame_tap_plan;

#define RSF_FRAME_TAP_DEPTH_DROP 0u
#define RSF_FRAME_TAP_DEPTH_KEEP 1u
#define RSF_FRAME_TAP_DEPTH_REFUSE 2u

/* Synchronous geometry observation. Pointers and arrays are borrowed only during the callback.
   Replay must happen here: retaining a buffer does not preserve its contents across later uploads.
   kind: 0 Draw, 1 DrawIndexed, 2 DrawInstanced, 3 DrawIndexedInstanced, 4 unsupported/indirect.
   Only draws with a depth view and one colour target are reported. */
typedef struct rsf_frame_tap_geometry {
    void* context;
    void* target;
    void* depth_view;
    uint32_t width, height, format, samples;
    uint32_t kind, count, start, instances, start_instance;
    int32_t base_vertex;
    void* vertex_buffers[32];
    uint32_t strides[32], offsets[32];
    void* index_buffer;
    uint32_t index_format, index_offset;
    void* input_layout;
    uint32_t topology;
    void* vertex_shader;
    void* vertex_constants[14];
} rsf_frame_tap_geometry;
typedef void (*rsf_frame_tap_geometry_fn)(void* user, const rsf_frame_tap_geometry* draw);

typedef struct rsf_frame_tap_options {
    uint32_t struct_size;
    uint32_t abi_version;
    rsf_frame_tap_fn on_pass;
    void* on_pass_user;
    rsf_frame_tap_log_fn log;
    void* log_user;
    /* Size in bytes of the constant buffer to watch for on the pixel stage, offered back as
       `view_constants`. Zero means 4096, which is what the AC7 view uniform buffer measures. It is
       a parameter because which buffer carries view data belongs to the game and its engine
       version, not to this module. */
    uint32_t view_constant_bytes;
    /* The presented resolution, which is what sizes are judged against.

       Deriving it from the bound set instead does not work, and failing to do so is what made the
       first version of this recognise nothing at all. Taking the largest bound texture as the
       render size makes it an exact requirement, so a single full resolution texture bound
       alongside the half resolution scene targets rejects every one of them. Judging against the
       presented size lets the classifier accept anything from half of it upwards that keeps the
       frame's aspect ratio, which is what a scaled render target is.

       Zero leaves this module unable to judge a size, and the classifier refuses rather than
       guessing, so a caller that does not know the presented size yet will see no passes. */
    uint32_t output_width;
    uint32_t output_height;
    /* Find the draws that read a texture of a given shape, wherever in the frame they are.

       The watches above start from a texture whose address is already known. This starts from a
       shape instead, which is what is needed when the question is "which surface is the interface"
       and every rule based on format has been wrong once. AC7's widget converter rasterizes at a
       hardcoded 1920x1080, so a draw reading a 1920x1080 R8G8B8A8 texture is compositing the
       interface, whatever the target it writes into turns out to be. That target is the answer.

       Zero in any of the three disables it. Matching costs no device calls: every shader resource
       slot already carries its description in the shadow, so this is a comparison over data the tap
       has anyway. Reports go to `on_hunt_draw` with the same structure a watched target draw uses,
       so the reader sees the render target, its extent, the viewport and every bound input. */
    uint32_t hunt_width;
    uint32_t hunt_height;
    uint32_t hunt_format;
    rsf_frame_tap_target_fn on_hunt_draw;
    void* on_hunt_draw_user;
    /* How many hunt reports to make before going quiet. Zero means the default, which is enough to
       describe a few frames and not enough to fill a log. */
    uint32_t hunt_budget;
    /* Where watched render target draws are reported. Optional: leaving it null leaves
       `rsf_frame_tap_watch_target` with nowhere to send anything, and it says so. */
    rsf_frame_tap_target_fn on_target_draw;
    void* on_target_draw_user;
    /* Reports draws reading the texture named by watch_input, independently of target watches.
       Only the installed context is observed by this watch. */
    rsf_frame_tap_target_fn on_input_draw;
    void* on_input_draw_user;
    rsf_frame_tap_geometry_fn on_geometry;
    void* on_geometry_user;
    /* Appended in ABI 7. Every draw that passes the candidate prefilter, wherever it draws and
       whether or not anything is being watched. This is the one report that is about the frame as a
       whole rather than about a target somebody named, and it is what answers which draws are the
       interface. Silent until `rsf_frame_tap_set_candidates` has been given something to match. */
    rsf_frame_tap_target_fn on_candidate_draw;
    void* on_candidate_draw_user;
} rsf_frame_tap_options;

typedef struct rsf_frame_tap_status {
    uint32_t struct_size;
    uint32_t installed;
    /* Calls that reached the hook with enough views bound to be worth examining. Calls rejected by
       the view count early out are not counted, since counting them would put an atomic write on
       the game's hottest binding path for nothing. */
    /* Every intercepted binding call, and the subset that actually changed what was bound.
       Separating them matters: a zero in the first says the hook never ran, a zero in the second
       with a non-zero first says it ran and never saw a change, and those are different faults
       with the same symptom of nothing happening. */
    uint32_t calls_seen;
    uint32_t calls_inspected;
    uint32_t passes_seen;
    /* How many times each role has been recognised in a binding. When no pass ever matches, these
       say which of the three the signature is waiting for, which is otherwise indistinguishable
       from the hook not working at all. */
    uint32_t motion_seen;
    uint32_t depth_seen;
    uint32_t exposure_seen;
    /* From the most recent qualifying pass. Zero until one is seen. */
    uint32_t render_width;
    uint32_t render_height;
    /* Draws into a watched render target that were reported. A watch that is set and stays at zero
       says the texture handed over is never drawn into, which is a different fault from a watch
       that reports and describes nothing recognisable. */
    uint32_t target_draws_reported;
    /* Whether a plan is set, and what it has done. Bindings altered and render targets redirected
       are counted apart because they fail differently: a plan that substitutes shader resources and
       redirects nothing draws a reconstruction into a render resolution target, and a plan that
       redirects and substitutes nothing draws the render resolution scene into an output resolution
       one. Both look like "it did something" from one number. */
    uint32_t plan_set;
    uint32_t inputs_substituted;
    uint32_t targets_redirected;
    uint32_t gates_opened;
    /* Bindings where a substituted target arrived with a depth stencil of a different size, and
       what was done about them. A promoted target is at output resolution and the game's depth is
       still at render resolution, so forwarding both is a pair D3D11 rejects and every draw in that
       pass is dropped. Counted apart from the redirects because a frame can redirect thousands of
       times and mismatch on the handful of passes that carry depth, which is exactly the case that
       loses geometry while leaving flat interface draws alone. The last sizes are kept so the
       report can name the pair rather than only count it. */
    uint32_t depth_mismatches;
    uint32_t depth_mismatch_target_width;
    uint32_t depth_mismatch_target_height;
    uint32_t depth_mismatch_depth_width;
    uint32_t depth_mismatch_depth_height;
    uint32_t depth_mismatch_depth_format;
    /* Appended in ABI 7. */
    uint32_t candidate_draws;
    uint32_t draws_diverted;
    uint32_t blend_states_patched;
    uint32_t divert_refused;
    uint32_t divert_last_refusal;
} rsf_frame_tap_status;

/* Patch the device context vtable. `device_context` is the immediate `ID3D11DeviceContext*`.
   Other contexts share the vtable but are forwarded without observation or substitution, so
   deferred bindings cannot contaminate the single-context shadow.

   Safe to call from a worker thread. It touches no D3D state, only the vtable pages. */
rsf_frame_tap_result rsf_frame_tap_install(void* device_context,
                                           const rsf_frame_tap_options* options);

/* Restore the original vtable entries and release what is retained.

   This cannot make a call already inside the hook finish first, so `on_pass` can still run once
   after this returns, and the callback and its user pointer have to stay valid until the caller
   knows the render thread has left. Uninstalling while the game is rendering is the same race the
   observer has, and the same mitigation applies: do it from a point where the render thread is
   known to be elsewhere, or leave it installed for the process's life. */
rsf_frame_tap_result rsf_frame_tap_uninstall(void);

/* Report the next `limit` draws into `texture`, an `ID3D11Texture2D*`, through `on_target_draw`.

   `index` is below `RSF_FRAME_TAP_WATCH_SLOTS`. A null texture clears that slot. `limit` is a
   budget rather than a preference: this runs on the render thread and a target drawn into a
   thousand times a frame would otherwise cost the game a callback per draw for a question that is
   answered by the first few. Zero means no limit and is for a caller that has its own.

   The texture is not retained. It is compared by pointer and never dereferenced by this module, so
   a caller that lets it go while a watch is live risks matching a later texture at the same
   address, not a use after free. Hold a reference for as long as the watch is set.

   Setting a slot resets its budget and its per-target draw ordinal. Safe to call from any thread,
   including from inside `on_target_draw`, which is how the second question follows the first. */
rsf_frame_tap_result rsf_frame_tap_watch_target(uint32_t index, void* texture, uint32_t limit);

/* Observe draws with a pixel shader SRV onto texture. One persistent watch, independent of the
   two target watches. Set/clear only on the render thread, including from on_pass. The caller
   retains texture while armed. No allocations or resource copies are made. Null clears it.
   Reports use watch_index == RSF_FRAME_TAP_WATCH_SLOTS. Bindings establish possible reads, not
   shader identity. A caller must qualify the draw before assigning meaning to its output. */
rsf_frame_tap_result rsf_frame_tap_watch_input(void* texture);

/* Start substituting, or stop. A null plan clears it and the game's frame goes back to being its
   own; so does uninstalling.

   Every view in the plan has to outlive it, and clearing the plan does not put back a binding that
   is already in place. Clear it and let a frame pass before releasing anything it named.

   Setting a plan changes what the game draws, which nothing else in this module does. It is refused
   unless the plan is complete enough to be coherent: a substitution with a texture and no
   replacement, or a viewport scale of zero, is `RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT` rather than a
   frame that half works. Safe to call from any thread; it takes effect on the next binding. */
rsf_frame_tap_result rsf_frame_tap_set_plan(const rsf_frame_tap_plan* plan);

/* Give the shape hunt a fresh budget, so it describes the frame that is on screen now.

   The surfaces it finds are pooled allocations named by address, and a screen change retires them.
   Whatever was found during an intro is not what a title screen composites into, and a hunt whose
   budget ran out during the intro will never say so: it simply stops reporting, and the stale set
   goes on being promoted while nothing is drawn into it. Anything that re-identifies the frame has
   to re-run this too. */
rsf_frame_tap_result rsf_frame_tap_reset_hunt(uint32_t budget);

/* Close every gate a plan opened, so the next frame opens them again.

   Called from the caller's own per frame point, normally the present hook, because this module has
   no idea where a frame ends: it watches bindings and draws, and nothing in either says so. */
rsf_frame_tap_result rsf_frame_tap_end_frame(void);

rsf_frame_tap_result rsf_frame_tap_get_status(rsf_frame_tap_status* status);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_FRAME_TAP_H */
