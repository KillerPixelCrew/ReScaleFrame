/* SPDX-License-Identifier: MIT */
/* Observe the game's D3D11 immediate context while it renders.

   The tap patches the context's vtable and shadows what the game binds, so a caller can see each
   geometry draw (`on_geometry`), each draw and dispatch with its pixel inputs and constant buffers
   (the research callbacks), and each constant buffer upload (the constant observer), and can bias
   the pixel samplers (`rsf_frame_tap_set_sampler_bias`). Nothing the game binds is altered except
   the sampler bias. Callbacks run synchronously on the game's render thread, the one moment the
   resources hold this frame's contents. */

#ifndef RSF_FRAME_TAP_H
#define RSF_FRAME_TAP_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* 16: the binding matcher, target and input watches, candidates, divert, constant override,
   substitution plans, upload provenance and status were removed with the renderer that used them. */
#define RSF_FRAME_TAP_ABI_VERSION 16u

/* Pixel shader resources reported per draw. A draw binding more is reported truncated, with
   `inputs_truncated` saying so. */
#define RSF_FRAME_TAP_MAX_INPUTS 16u

typedef int32_t rsf_frame_tap_result;
#define RSF_FRAME_TAP_OK ((rsf_frame_tap_result)0)
#define RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT ((rsf_frame_tap_result)-1)
#define RSF_FRAME_TAP_ERROR_ABI_MISMATCH ((rsf_frame_tap_result)-2)
#define RSF_FRAME_TAP_ERROR_ALREADY_INSTALLED ((rsf_frame_tap_result)-3)
#define RSF_FRAME_TAP_ERROR_NOT_INSTALLED ((rsf_frame_tap_result)-4)
#define RSF_FRAME_TAP_ERROR_PATCH_FAILED ((rsf_frame_tap_result)-5)

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

/* One draw the research callbacks are offered. A description of the game's own draw; nothing about
   it is altered. */
typedef struct rsf_frame_tap_target_draw {
    uint32_t struct_size;
    /* The `ID3D11DeviceContext*` that made the call. */
    void* context;
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
    /* Further facts of the draw. */
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
    /* Appended in ABI 8. The vertex stage's constant buffers as bound for this draw, by slot,
       `ID3D11Buffer*` compared by address only. */
    void* vertex_constants[14];
    /* ABI 12: pixel-stage buffers, in the same slot order. */
    void* pixel_constants[14];
    /* ABI 13: the later graphics stages. */
    void* geometry_constants[14];
    void* hull_constants[14];
    void* domain_constants[14];
} rsf_frame_tap_target_draw;

/* Called on the render thread, around the game's own draw. */
typedef void (*rsf_frame_tap_target_fn)(void* user, const rsf_frame_tap_target_draw* draw);

typedef void (*rsf_frame_tap_compute_fn)(void* user, void* context, uint32_t x, uint32_t y,
                                       uint32_t z, void* indirect_arguments, uint32_t offset);
/* Texture LOD bias for pixel-shader samplers that blend between mips, applied to the observed
   context from now until it is set back to zero. Biased clones replace the game's samplers in
   every slot already bound and in every later PSSetSamplers; zero restores the game's own. Call on
   the observed context's thread, at a point in its command stream, such as a queued scope. This is
   the mechanism only: which passes and how much belong to the caller. */
rsf_frame_tap_result rsf_frame_tap_set_sampler_bias(float bias);

/* Capture observers: paired observations around Draw/DrawIndexed, and compute work reported after
   dispatch. Before and after use the same target draw ordinal. Draw facts describe the game's
   requested state. Set or clear on the observed context's render thread between frames; passing
   null clears. The callbacks observe the immediate context and must preserve its bindings. */
rsf_frame_tap_result rsf_frame_tap_set_research_phase_callbacks(rsf_frame_tap_target_fn before,
                                                              rsf_frame_tap_target_fn after,
                                                              rsf_frame_tap_compute_fn compute,
                                                              void* user);

/* Called with the contents of a constant buffer the game uploads with Map(WRITE_DISCARD), before
   the Unmap is forwarded, and of a whole-buffer UpdateSubresource before it is forwarded. Unreal
   4.18's D3D11 RHI writes every pooled uniform buffer that way. `contents` is borrowed for the
   callback and must not be written. Render thread; must not call into the context.

   `bytes` names the buffers to watch: a width, or zero for every constant buffer of up to 4096
   bytes. A null callback disarms the watch, which should be done when it is not needed: it costs a
   lookup on every constant upload the game makes. */
typedef void (*rsf_frame_tap_constant_observer_fn)(void* user, void* buffer, const void* contents,
                                                    uint32_t bytes);
rsf_frame_tap_result rsf_frame_tap_set_constant_observer(uint32_t bytes,
                                                         rsf_frame_tap_constant_observer_fn fn,
                                                         void* user);

/* Synchronous geometry observation. Pointers and arrays are borrowed only during the callback.
   Replay must happen here: retaining a buffer does not preserve its contents across later uploads.
   kind: 0 Draw, 1 DrawIndexed, 2 DrawInstanced, 3 DrawIndexedInstanced, 4 DrawAuto,
   5 DrawIndexedInstancedIndirect, 6 DrawInstancedIndirect.
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
    /* Borrowed at this exact draw; argument contents can be produced on the GPU. */
    void* indirect_buffer;
    uint32_t indirect_offset;
} rsf_frame_tap_geometry;
typedef void (*rsf_frame_tap_geometry_fn)(void* user, const rsf_frame_tap_geometry* draw);

typedef struct rsf_frame_tap_options {
    uint32_t struct_size;
    uint32_t abi_version;
    rsf_frame_tap_geometry_fn on_geometry;
    void* on_geometry_user;
} rsf_frame_tap_options;

/* Patch the device context vtable. `device_context` is the immediate `ID3D11DeviceContext*`.
   Other contexts share the vtable but are forwarded without observation, so
   deferred bindings cannot contaminate the single-context shadow.

   Safe to call from a worker thread. It touches no D3D state, only the vtable pages. */
rsf_frame_tap_result rsf_frame_tap_install(void* device_context,
                                           const rsf_frame_tap_options* options);

/* Restore the original vtable entries and release what is retained.

   Calls already inside the hooks on other threads finish before anything is released, and new ones
   wait until this returns. Called from inside one of this module's callbacks, it never waits: it is
   refused with `RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT`, and nothing changes, when another thread is
   inside the hooks or in another install or uninstall. When it succeeds there, the hook
   that made the callback still finishes on that thread, so callbacks and their user pointers have
   to stay valid until it returns. If a slot write fails, this returns RSF_FRAME_TAP_ERROR_PATCH_FAILED.
   Outstanding detours only forward. Retry uninstall to restore them before installing again. */
rsf_frame_tap_result rsf_frame_tap_uninstall(void);

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

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_FRAME_TAP_H */
