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
   scene colour, which is the one to intervene at. Nothing here changes a binding or a draw. */

#ifndef RSF_FRAME_TAP_H
#define RSF_FRAME_TAP_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_FRAME_TAP_ABI_VERSION 3u

/* Render targets watched at once. Two, because the question this answers needs exactly two: the
   swap chain's back buffer, and whichever target the draw into it reads. */
#define RSF_FRAME_TAP_WATCH_SLOTS 2u

/* Pixel shader resources reported per watched draw. Sixteen covers every post process pass in this
   game's frame; a pass binding more is reported truncated, with `input_count` saying so, rather
   than not reported at all. */
#define RSF_FRAME_TAP_MAX_INPUTS 16u

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
    /* The `ID3D11DeviceContext*` that made the call. Deferred contexts share the immediate
       context's vtable, so this is the one to record commands on, not the immediate context. */
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
    /* Non-zero for `DrawIndexed`. A full screen pass in this engine is an unindexed draw of three
       or six vertices, so the pair distinguishes one from interface geometry without guessing. */
    uint32_t indexed;
    uint32_t element_count;
    /* Occupied pixel shader slots, and the first `RSF_FRAME_TAP_MAX_INPUTS` of them. `input_count`
       is what was reported, not what was bound, when the two differ. */
    uint32_t input_count;
    const rsf_frame_tap_input* inputs;
} rsf_frame_tap_target_draw;

/* Called on the render thread, immediately after the game's own draw has been forwarded. */
typedef void (*rsf_frame_tap_target_fn)(void* user, const rsf_frame_tap_target_draw* draw);

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
    /* Where watched render target draws are reported. Optional: leaving it null leaves
       `rsf_frame_tap_watch_target` with nowhere to send anything, and it says so. */
    rsf_frame_tap_target_fn on_target_draw;
    void* on_target_draw_user;
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
} rsf_frame_tap_status;

/* Patch the device context vtable. `device_context` is an `ID3D11DeviceContext*`; the immediate
   context is the one to pass, and the patch applies to every context the runtime creates because
   they share the vtable.

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

rsf_frame_tap_result rsf_frame_tap_get_status(rsf_frame_tap_status* status);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_FRAME_TAP_H */
