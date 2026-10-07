/* SPDX-License-Identifier: GPL-3.0-only */
/* Observes render work on one immediate D3D11 context and can optionally substitute selected
   resources and redirect render targets. Callbacks run synchronously on the render thread; all
   callback pointers are borrowed unless a field says otherwise. AC7 pass-classification evidence
   and limits are recorded in docs/research/ac7-frame-capture.md. */

#ifndef RSF_FRAME_TAP_H
#define RSF_FRAME_TAP_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_FRAME_TAP_ABI_VERSION 13u
#define RSF_FRAME_TAP_CONSTANT_SLOTS 70u

/* Maximum simultaneous render-target watches. */
#define RSF_FRAME_TAP_WATCH_SLOTS 4u

/* Maximum pixel-shader inputs in one report; detailed reports flag excess inputs as truncated. */
#define RSF_FRAME_TAP_MAX_INPUTS 16u

/* Maximum resource substitutions in one plan. */
#define RSF_FRAME_TAP_MAX_SUBSTITUTIONS 12u

typedef int32_t rsf_frame_tap_result;
#define RSF_FRAME_TAP_OK ((rsf_frame_tap_result)0)
#define RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT ((rsf_frame_tap_result)-1)
#define RSF_FRAME_TAP_ERROR_ABI_MISMATCH ((rsf_frame_tap_result)-2)
#define RSF_FRAME_TAP_ERROR_ALREADY_INSTALLED ((rsf_frame_tap_result)-3)
#define RSF_FRAME_TAP_ERROR_NOT_INSTALLED ((rsf_frame_tap_result)-4)
#define RSF_FRAME_TAP_ERROR_PATCH_FAILED ((rsf_frame_tap_result)-5)

/* Optional diagnostics sink. */
typedef void (*rsf_frame_tap_log_fn)(void* user, const char* message);

/* One heuristic reconstruction-input set. Texture pointers borrow shadow-owned references for
   the callback; retain separately to outlive it, or copy at a chosen consumption point. Retaining a
   pooled texture does not preserve its contents or establish the input's frame/view identity. */
typedef struct rsf_frame_tap_pass {
    uint32_t struct_size;
    /* The installed `ID3D11DeviceContext*` that made the call. */
    void* context;
    /* Selected floating colour texture matching the motion extent and render-target binding.
       Selection is a descriptor/binding heuristic, not a verified scene/view identity. */
    void* scene_color;
    /* Optional RGBA16F scene-colour-shaped candidate from the binding scan. Its contents are
       not verified as accumulated history; null if no such candidate was bound. */
    void* history;
    /* `ID3D11Texture2D*`, the engine's velocity target, still in the engine's own encoding. */
    void* motion;
    /* `ID3D11Texture2D*`. */
    void* depth;
    /* Optional 1x1 eye-adaptation-shaped texture. Exposure is not required for qualification. */
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

/* Application draw facts used by watches, research callbacks, verdicts and constant overrides.
   Identities describe requested game state; resource substitution or temporary overrides may
   change the actual draw. Detailed reports query the effective viewport after override restoration.
   Pre-draw verdict/constant facts omit context, viewport, ordinal and truncation diagnostics. */
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
    /* Zero-based ordinal since the requested RTV identity last changed. Rebinding the same view
       continues the ordinal; changing views resets it, even if both reference one texture. */
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
    /* The bound render target view's format, which is not the texture's when the texture is
       typeless. Unreal allocates typeless and chooses sRGB per view, so this is the only thing that
       says whether a draw's colour is being encoded on the way in. A layer that does not encode
       where the original did holds linear values that later read as too dark. */
    uint32_t target_view_format;
    /* Appended in ABI 8. Borrowed ID3D11Buffer identities by slot. Constant arrays are populated
       for verdict/constant-override and research callbacks, not ordinary watch/candidate reports. */
    void* vertex_constants[14];
    /* ABI 12: pixel-stage view buffers for per-draw overrides, in the same slot order. */
    void* pixel_constants[14];
    /* ABI 13: the later graphics stages, filled for constant overrides. */
    void* geometry_constants[14];
    void* hull_constants[14];
    void* domain_constants[14];
} rsf_frame_tap_target_draw;

/* Called on the render thread, immediately after the game's own draw has been forwarded. */
typedef void (*rsf_frame_tap_target_fn)(void* user, const rsf_frame_tap_target_draw* draw);

/* Temporary capture observers, independent of UI candidates and target watches. Set/clear on
   the observed context's render thread between frames. Draw facts describe requested game state
   after existing overrides are restored; compute work is reported after dispatch. */
typedef void (*rsf_frame_tap_compute_fn)(void* user, void* context, uint32_t x, uint32_t y,
                                       uint32_t z, void* indirect_arguments, uint32_t offset);
rsf_frame_tap_result rsf_frame_tap_set_research_callbacks(rsf_frame_tap_target_fn draw,
                                                        rsf_frame_tap_compute_fn compute,
                                                        void* user);
/* Texture LOD bias for pixel-shader samplers that blend between mips, applied to the observed
   context from now until it is set back to zero. Biased clones replace the game's samplers in
   every slot already bound and in every later PSSetSamplers; zero restores the game's own. Call on
   the observed context's thread, at a point in its command stream, such as a queued scope. This is
   the mechanism only: which passes and how much belong to the caller. */
rsf_frame_tap_result rsf_frame_tap_set_sampler_bias(float bias);

/* Optional paired observations around Draw/DrawIndexed. Before and after use the same target
   draw ordinal. These callbacks observe the immediate context and must preserve its bindings. */
rsf_frame_tap_result rsf_frame_tap_set_research_phase_callbacks(rsf_frame_tap_target_fn before,
                                                              rsf_frame_tap_target_fn after,
                                                              rsf_frame_tap_compute_fn compute,
                                                              void* user);

/* Pointer sets for a cheap draw prefilter. Arrays are copied and can be released after the call;
   objects are borrowed identities and must be invalidated on reuse. Replace sets on the observed
   render thread, or with its readers quiescent; publication atomics do not retire prior readers. */
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
/* Divert, enable alpha writes, and set enabled blends' alpha factors to One/InvSrcAlpha/Add.
   Unreal's Zero/InvSrcAlpha base-pass blend otherwise leaves coverage zero on a cleared layer.
   Colour factors and blend enable are unchanged; an already accumulating blend needs no patch. */
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

/* Arm or disarm diverting. Null, or a null layer or verdict, disarms. Coordinate setup changes
   with the observed render thread; keep borrowed layer/callback storage alive until old draws exit.

   Diversion remains disabled until armed. */
rsf_frame_tap_result rsf_frame_tap_set_divert(const rsf_frame_tap_divert_setup* setup);

/* Asked before a candidate draw is forwarded: which constant buffers to replace for that draw
   only. Fill `slots` and `buffers` (`ID3D11Buffer*`), up to RSF_FRAME_TAP_CONSTANT_SLOTS pairs.
   Consecutive groups of 14 name VS, PS, GS, HS and DS b0..b13. Return the pair count; zero or
   a negative value leaves the draw alone. Duplicate/out-of-range slots and null buffers are ignored.

   For drawing the interface with an unjittered copy of its view's uniform buffer. The tap binds
   each buffer through the original entry, forwards the draw, and puts the game's own buffers back,
   so its shadow and the game's state stay as the game set them. Counted in `draws_overridden`.
   Independent of diverting; a diverted draw is not asked. Appended in ABI 8; several slots since
   ABI 10; pixel-stage slots since ABI 12; GS/HS/DS since ABI 13. */
typedef int (*rsf_frame_tap_constant_override_fn)(void* user, const rsf_frame_tap_target_draw* draw,
                                                  uint32_t* slots, void** buffers);
rsf_frame_tap_result rsf_frame_tap_set_constant_override(rsf_frame_tap_constant_override_fn fn,
                                                         void* user);

/* Also offer draws writing this DXGI_FORMAT to the callback. Zero disables this prefilter.
   Allows the first draw after allocation/reuse to be classified before rasterization. The
   callback must validate the remaining draw facts; a format alone does not identify a pass. */
void rsf_frame_tap_set_constant_override_format(uint32_t format);

/* Offer the constant override for every draw into `texture`, an `ID3D11Texture2D*`, as well as for
   the candidates. Four slots; a null texture clears one. For scene geometry whose projection must
   not carry the jitter, such as a separate translucency layer composited after the reconstruction. */
rsf_frame_tap_result rsf_frame_tap_set_override_target(uint32_t index, void* texture);

/* Called with the contents of a constant buffer the game uploads with Map(WRITE_DISCARD), before
   the Unmap is forwarded. Unreal 4.18's D3D11 RHI writes every pooled uniform buffer that way
   (`D3D11UniformBuffer.cpp:168`), so this is each view's uniform buffer as the frame fills it, and
   it sends each shader's own constants with UpdateSubresource from its CPU shadow, a whole
   sub-buffer sized to the upload (`WindowsD3D11ConstantBuffer.cpp:90`), right before the draw
   that uses them; the watch sees those on a copy that is then uploaded in the shadow's place. `contents` is the
   mapped memory itself and `bytes` the buffer's width: the callback may write into it, and what it
   writes is what the game's draw reads. Render thread; must not call into the context.

   `bytes` names the buffers to watch: a width, or zero for every constant buffer of up to 4096
   bytes. Null disarms. */
typedef void (*rsf_frame_tap_constants_fn)(void* user, void* buffer, void* contents,
                                           uint32_t bytes);
rsf_frame_tap_result rsf_frame_tap_set_constant_watch(uint32_t bytes, rsf_frame_tap_constants_fn fn,
                                                      void* user);

/* The `ID3D11Texture2D*` behind the render target the game has at output slot 0, as the game bound
   it, or null. Borrowed, not retained; render thread only. This is how a constant watch tells which
   draw an upload belongs to: the engine binds the target, then fills the constants, then draws. */
void* rsf_frame_tap_bound_target(void);

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

/* Called when a substitution's `after_target` is bound as a single render target, before that
   binding is forwarded. One callback covers all closed gates waiting on that target. MRT binds
   never open these gates: a pooled post-process target can still be a GBuffer earlier in the frame.

   The owner identifies the finished-scene boundary and restores any changed context bindings.
   Return nonzero to open the waiting gates; zero leaves them closed for a later binding retry. */
typedef int (*rsf_frame_tap_gate_fn)(void* user, void* context, void* texture);

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
    /* Policy for depth whose measured extent differs from the promoted target:
       DROP removes the depth view and preserves the colour substitution, losing scene occlusion;
       KEEP forwards the mismatched pair and may produce an invalid D3D11 binding;
       REFUSE keeps the native target for that binding, possibly bypassing promoted downstream data.
       DROP is zero/default. Matching-size depth with correct contents is the owner's responsibility. */
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
    /* Presented extent used for descriptor size/aspect judgment. Zero prevents qualifying sets.
       Auxiliary full-size bindings must not be mistaken for the render resolution. */
    uint32_t output_width;
    uint32_t output_height;
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
    /* Requested PSSetShaderResources calls on the observed context, excluding hook reentry. */
    uint32_t calls_seen;
    /* Calls that changed at least one tracked SRV slot. */
    uint32_t calls_inspected;
    uint32_t passes_seen;
    /* Cumulative descriptor-role observations, including repeated bindings. Exposure is optional
       for input qualification; there is no corresponding scene-colour counter in this ABI. */
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
    /* Appended in ABI 8. How many times the runtime rewrote its vtable underneath the hooks and
       they were put back. Zero under DXVK, whose table is static; on Windows it climbs with every
       flush, and a run where it stays at zero while draws go unobserved is a run on a runtime this
       module has not met. See `rsf_frame_tap_refresh`. */
    uint32_t vtable_refreshes;
    /* Draws given one or more temporary VS/PS/GS/HS/DS constant replacements. */
    uint32_t draws_overridden;
    /* CopyResource calls between two promoted textures sent between their stand-ins, and calls
       with only one side promoted, which D3D11 would drop for the size difference. */
    uint32_t copies_redirected;
    uint32_t copies_mismatched;
    /* Appended in ABI 9: constant uploads by UpdateSubresource handed to the watch. */
    uint32_t updates_watched;
    /* Appended in ABI 11: gate bindings the callback declined. */
    uint32_t gates_declined;
} rsf_frame_tap_status;

/* Patch the device context vtable. `device_context` is the immediate `ID3D11DeviceContext*`.
   Other contexts share the vtable but are forwarded without observation or substitution, so
   deferred bindings cannot contaminate the single-context shadow.

   The context is borrowed and must outlive installation. A worker can install at a point where
   the observed render thread is quiescent; installation changes vtable pages, not D3D bindings.
   Callback functions/user storage must outlive in-flight hooks, including teardown. */
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

   Setting a slot resets its budget; ordinal follows target-binding changes, not watch setup.
   Safe to call from any thread,
   including from inside `on_target_draw`, which is how the second question follows the first. */
rsf_frame_tap_result rsf_frame_tap_watch_target(uint32_t index, void* texture, uint32_t limit);

/* Observe draws with a pixel shader SRV onto texture. One persistent watch, independent of the
   target watches. Set/clear only on the render thread, including from on_pass. The caller
   retains texture while armed. No allocations or resource copies are made. Null clears it.
   Reports use watch_index == RSF_FRAME_TAP_WATCH_SLOTS. Bindings establish possible reads, not
   shader identity. A caller must qualify the draw before assigning meaning to its output. */
rsf_frame_tap_result rsf_frame_tap_watch_input(void* texture);

/* Start substituting, or stop. A null plan clears it and the game's frame goes back to being its
   own; so does uninstalling.

   Every view in the plan has to outlive it, and clearing the plan does not put back a binding that
   is already in place. Clear it and let a frame pass before releasing anything it named.

   A plan is refused unless it is complete enough to be coherent: a substitution with a texture and no
   replacement, or a viewport scale of zero, is `RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT` rather than a
   incoherent plan. Set/clear on the observed render thread or while it is quiescent; the writer
   mutex and active flag do not retire an already-entered reader. It affects later bindings. */
rsf_frame_tap_result rsf_frame_tap_set_plan(const rsf_frame_tap_plan* plan);

/* Put the hooks back if the runtime has rewritten its vtable underneath them.

   The Windows D3D11 runtime keeps the immediate context's vtable on the heap and rewrites the
   whole work-submission family of entries, draws, dispatches, copies and clears, whenever a
   flush-class call runs and again on the next piece of work, flipping between two sets of
   implementations. Every rewrite discards whatever was patched into those slots. Measured on
   Windows 11 on 26 September 2026; DXVK's static table never does this, which is why every Wine
   run of this module passed and no Windows run observed a draw after the first Map.

   Every hook already checks for this on the way in and the work-submission hooks check again on
   the way out, so this exists for the one flip nothing here can see: the one Present causes.
   Call it from the present hook. Cheap when nothing changed: one comparison. */
rsf_frame_tap_result rsf_frame_tap_refresh(void);

/* Close every gate a plan opened on the observed render thread, so the next frame opens them again.

   Called from the caller's own per frame point, normally the present hook, because this module has
   no idea where a frame ends: it watches bindings and draws, and nothing in either says so. */
rsf_frame_tap_result rsf_frame_tap_end_frame(void);

/* Copy atomic diagnostic counters and latest extents. Caller initializes status.struct_size;
   counters can advance during the read, so this is not one transactional frame snapshot. */
rsf_frame_tap_result rsf_frame_tap_get_status(rsf_frame_tap_status* status);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_FRAME_TAP_H */
