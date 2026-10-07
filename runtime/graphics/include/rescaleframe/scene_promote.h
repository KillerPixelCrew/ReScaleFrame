/* SPDX-License-Identifier: GPL-3.0-only */
/* Build output-resolution replacements for caller-identified post-process and interface targets.
   A substitution plan preserves the game's grading/compositing passes while scaling viewports and
   scissors to the replacement extent. Game-specific target identification belongs to the caller.

   Scene-colour substitution opens only at the composite or recombine gate, after scene rendering
   is complete. Earlier substitution would feed unfinished lighting with reconstructed output.
   On the recombine route, seed the scene stand-in before recombination, route subsequent native
   TAA writes to scratch, and finish by copying the recombined result to the tonemap's scene input.

   Original textures and optional layer_view remain caller-owned throughout plan use. Replacements
   belong to rsf_promote; clear the tap's plan before preparing again or destroying them. Use the
   render thread. Promotion does not update all engine shader constants or the low-resolution
   bloom chain. The tap's depth policy determines occlusion when depth and promoted extents differ.
   See docs/research/ac7-ui-composition.md and ac7-ui-extraction.md for measured route selection. */

#ifndef RSF_SCENE_PROMOTE_H
#define RSF_SCENE_PROMOTE_H

#include <rescaleframe/frame_tap.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ABI 7 includes typed-view formats, the optional recombine route, and layer-view substitution. */
#define RSF_PROMOTE_ABI_VERSION 7u
/* How many interface layers may be promoted. AC7 alternates between two allocations from frame to
   frame on the briefing, and a screen with more of them should lose none rather than lose the ones
   found last. */
#define RSF_PROMOTE_MAX_UI_TARGETS 4u
/* How many intermediates between the tonemap and the interface composite may be promoted. The
   briefing tail has one; two leaves room for it to alternate. */
#define RSF_PROMOTE_MAX_CHAIN_TARGETS 2u

typedef int32_t rsf_promote_result;
#define RSF_PROMOTE_OK ((rsf_promote_result)0)
#define RSF_PROMOTE_ERROR_INVALID_ARGUMENT ((rsf_promote_result)-1)
#define RSF_PROMOTE_ERROR_ABI_MISMATCH ((rsf_promote_result)-2)
/* A replacement texture or one of its views could not be created. */
#define RSF_PROMOTE_ERROR_RESOURCE_FAILED ((rsf_promote_result)-3)
/* Render extent exceeds output extent rounded up to four pixels. Equal native/DLAA extents
   are accepted; this code refuses a downsampling route. */
#define RSF_PROMOTE_ERROR_NOT_SCALED ((rsf_promote_result)-4)

typedef void (*rsf_promote_log_fn)(void* user, const char* message);

typedef struct rsf_promote_setup {
    uint32_t struct_size;
    uint32_t abi_version;
    /* The game's `ID3D11Device*`, retained until destroy. Its own device, because the replacements
       have to be usable in the game's own draws. */
    void* device;
    /* Presented resolution, which is what everything here is promoted to. */
    uint32_t output_width;
    uint32_t output_height;
    rsf_promote_log_fn log;
    void* log_user;
} rsf_promote_setup;

/* The part of the frame this substitutes into, as the caller found it. Every texture is an
   `ID3D11Texture2D*` belonging to the game, borrowed for the call and not retained: this reads
   their descriptors and keeps their addresses, so a caller has to keep them alive itself for as
   long as the plan is in use. Null entries in either array are skipped. */
typedef struct rsf_promote_frame_tail {
    uint32_t struct_size;
    /* The render resolution target the tonemap writes and the last draw reads. Required. */
    void* composite;
    /* Classifier-identified interface layers. An empty set leaves interface magnification to
       the engine; do not infer targets from descriptor shape alone. */
    void* ui_targets[RSF_PROMOTE_MAX_UI_TARGETS];
    uint32_t ui_target_count;
    /* Intermediates confirmed by draws between the tonemap and interface composite. An empty
       set can leave a render-resolution downsample in the promoted path. */
    void* chain_targets[RSF_PROMOTE_MAX_CHAIN_TARGETS];
    uint32_t chain_target_count;
    /* The scene colour the tonemap reads, and what replaces it. */
    void* scene_color;
    /* The reconstruction, at output resolution. `rsf_dlss_pipeline_output_texture` is one. */
    void* reconstruction;
    /* What the game is rendering at, which is what viewports have to be scaled from. */
    uint32_t render_width;
    uint32_t render_height;
    /* The DXGI format of the render target view the game binds each surface with, as the frame
       tap reports it. Unreal allocates its targets typeless and decides sRGB or not per view, so
       the replacement's views take this format rather than the texture's; zero means derive it,
       which maps a typeless family to its plain UNORM member and leaves a typed format alone.
       Getting it wrong is not a failure but a picture with the wrong transfer curve. */
    uint32_t composite_view_format;
    uint32_t ui_target_view_format;
    uint32_t chain_view_format;
    /* Appended in ABI 6. The render resolution target the game's recombine writes: scene colour
       with the separate translucency layer composited over it. Null keeps the older route, where
       the reconstruction includes translucency and replaces scene colour at the tonemap. */
    void* composed;
    /* Appended in ABI 7. The separate translucency layer the recombine reads, and the shader
       resource view (`ID3D11ShaderResourceView*`, the caller's, kept alive by the caller) to read
       in its place from the recombine onward. Either null leaves the layer as it is. */
    void* layer;
    void* layer_view;
} rsf_promote_frame_tail;

typedef struct rsf_promote_status {
    uint32_t struct_size;
    /* Whether a tail has been prepared and the plan is fit to hand to the frame tap. */
    uint32_t ready;
    /* How many interface layers are promoted. Zero means the interface is magnified with the
       scene. */
    uint32_t ui_targets_promoted;
    /* How many intermediates are promoted. Zero means the scene is downsampled between the tonemap
       and the interface composite, and the composite's promotion buys nothing visible. */
    uint32_t chain_targets_promoted;
    /* Appended in ABI 6: whether the reconstruction goes in at the recombine. */
    uint32_t at_recombine;
    uint32_t render_width;
    uint32_t render_height;
    uint32_t output_width;
    uint32_t output_height;
} rsf_promote_status;

typedef struct rsf_promote rsf_promote;

rsf_promote_result rsf_promote_create(const rsf_promote_setup* setup, rsf_promote** out);

/* Build the replacements for this tail. Safe to call again with a different tail, which is what a
   render scale change, a swap chain resize, or a newly seen ui target needs; the previous
   replacements are released, so clear the tap's plan first and set it again afterwards.

   Call it from the thread that drives the frames. It creates textures and views, which is not
   something to do from a key polling thread while the render thread is drawing. */
rsf_promote_result rsf_promote_prepare(rsf_promote* promote, const rsf_promote_frame_tail* tail);

/* Fill a plan for rsf_frame_tap_set_plan after successful prepare. Preserve only struct_size,
   zero the rest, and populate substitutions/viewport scale. The caller sets the gate callback
   and depth policy afterwards. Plan views borrow promotion-owned resources until prepare/destroy. */
rsf_promote_result rsf_promote_fill_plan(rsf_promote* promote, rsf_frame_tap_plan* plan);

rsf_promote_result rsf_promote_get_status(rsf_promote* promote, rsf_promote_status* status);

/* Fill scene colour's stand-in, on `context`, the immediate one. Call it at the gate, before the
   recombine reads scene colour, every time the gate opens; the caller saves and restores the
   context around it. Does nothing on the older route.

   With `reconstructed` nonzero the stand-in gets the reconstruction. With zero it gets the game's
   own render resolution scene colour stretched to output size, so a frame the reconstruction was
   refused for is soft rather than whatever the stand-in held before. */
rsf_promote_result rsf_promote_seed(rsf_promote* promote, void* context, uint32_t reconstructed);

/* Put the recombined result, reconstruction with full-size translucency over it, where the tonemap
   reads scene colour. Call it at the gate the composite opens, on the recombine route. Does nothing
   on the older route. */
rsf_promote_result rsf_promote_finish(rsf_promote* promote, void* context);

/* The output size stand-ins of the recombine route, `ID3D11Texture2D*`, borrowed: scene colour's,
   which the tonemap reads, and the recombined target's, which the game's recombine writes. Null
   on the older route. For writing them to disk when a frame has to be looked at. */
rsf_promote_result rsf_promote_get_stand_ins(rsf_promote* promote, void** scene, void** composed);

/* Clear the tap plan and quiesce its callbacks before releasing replacement resources. Null
   is accepted. Original identities and caller-supplied layer_view are not released here. */
void rsf_promote_destroy(rsf_promote* promote);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_SCENE_PROMOTE_H */
