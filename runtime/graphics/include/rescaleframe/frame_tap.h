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
   frame after it finished. */

#ifndef RSF_FRAME_TAP_H
#define RSF_FRAME_TAP_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_FRAME_TAP_ABI_VERSION 2u

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

rsf_frame_tap_result rsf_frame_tap_get_status(rsf_frame_tap_status* status);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_FRAME_TAP_H */
