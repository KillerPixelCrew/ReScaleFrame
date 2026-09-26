/* SPDX-License-Identifier: GPL-3.0-only */
/* Promote the tail of the game's frame to output resolution, so the reconstruction is graded by the
   game's own tonemap and the interface is drawn sharp by the game's own composite.

   The debug view in present_blit.h draws the reconstruction over the finished frame. That is how
   the result got looked at at all, and it is not how this should work: the image it shows is
   pre-tonemap scene colour, so it is ungraded, and everything the game composited afterwards is
   gone with it. The interface included.

   What the frame actually does at a reduced render scale, from the briefing tail a run log
   described in docs/research/ac7-ui-composition.md:

     scene colour (render res, HDR)
       -> tonemap and grade               -> composite (render res, eight bit)
       -> an intermediate copy            -> chain target (render res, eight bit)
     widget quads (world space geometry reading 1920x1080 widget textures, scene depth bound)
       -> AC7's own interface layer       -> ui target (render res, R8G8B8A8, premultiplied)
     the game's own interface composite: chain target + ui target + glow -> composite again
       -> one draw                        -> back buffer (output res)

   So the last draw is a spatial upscale of a picture that already has the interface in it, and the
   interface is content the engine keeps processing after the quads: glow, its own composite, the
   grade. Taking the quads out of the frame and compositing them at present skips all of that, which
   was measured on 7 September 2026 and is why extraction is not the route (see
   docs/research/ac7-ui-extraction.md). Promotion keeps every one of those passes and only changes
   the size of the surfaces they run on.

   The intervention is four substitutions and a scale:

     - the composite becomes an output resolution texture of the same format, so everything drawn
       into it lands at output resolution;
     - every ui target does too. The quads are drawn with the game's own render resolution viewport,
       which the frame tap scales while a promoted target is bound, so they rasterize at output
       resolution from their 1920x1080 sources rather than being squashed to render resolution and
       stretched back out by the last draw. This is what makes the interface sharp;
     - every chain target does too. An intermediate between the tonemap and the interface composite
       that stays at render resolution downsamples the promoted composite back to render resolution
       and the sharpness of the scene is lost on the way to the back buffer. Promoting the composite
       alone was game-tested on 7 September and made the picture cleaner without making it sharper,
       and this is why;
     - the scene colour the tonemap reads becomes the reconstruction, from the moment the composite
       is first bound in a frame and no earlier, because the scene passes read scene colour as well
       and handing them a reconstruction of the frame they are still drawing is a feedback loop;
     - viewports and scissor rectangles are scaled while a substituted target is bound, since the
       game asks for the render resolution it believes it is drawing at.

   The last draw then reads an output resolution composite and writes an output resolution back
   buffer, so the game's own upscale becomes a copy. Nothing is removed from the frame.

   Depth. The quads bind the scene's depth, which is at render resolution, and D3D11 refuses a
   target and a depth of different sizes. The frame tap's depth policy decides what happens at that
   binding and the default drops the depth, which gives the quads overlay semantics: they draw,
   without being occluded by scene geometry in front of a panel. That is right for the menus and
   wrong wherever occlusion was real, and the honest fix is a depth of the right size, which
   depth_replay can produce and nothing here uses yet.

   What this does not fix, and it is worth being plain about. The bloom chain is still computed from
   the render resolution scene, so the glow the tonemap composites over the reconstruction is low
   resolution. The quads read the scene and its blur chain at render resolution alongside the widget
   texture, so the glow around the interface is low resolution too. Post process shaders that do
   texel addressed work rather than normalised sampling will address the wrong texels, because their
   constants still describe the buffer the engine believes it has. All of it is visible only in a
   rendered result.

   Nothing here is game specific. It is given textures and sizes; which textures those are is the
   caller's question. In this repository the loader answers it by watching the frame's tail for the
   composite and the chain, and by asking the game's draw classifier which draws are widget quads,
   whose targets are the ui targets. */

#ifndef RSF_SCENE_PROMOTE_H
#define RSF_SCENE_PROMOTE_H

#include <rescaleframe/frame_tap.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* 4: the interface targets found by shape became the ui targets the classifier names, and the
   chain targets between the tonemap and the interface composite were added.
   5: the tail carries the view formats the game binds each surface with, because the surfaces are
   typeless and a view on a typeless texture has to be told its format; the first Windows run
   failed every promotion on exactly that. */
#define RSF_PROMOTE_ABI_VERSION 5u
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
/* The tail describes a frame that is already at output resolution, so there is nothing to promote
   into: substituting here would replace a texture with one of its own size and change nothing
   except the number of textures in the frame. */
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
    /* The render resolution layers the widget quads draw into: AC7's own interface targets, named
       by the draw classifier rather than by a rule about formats or shapes, because every rule of
       that form was wrong at least once in this frame. An empty set is allowed and means none was
       seen: the scene is still promoted and the interface is magnified with it, which is better
       than substituting a texture that might be something else. */
    void* ui_targets[RSF_PROMOTE_MAX_UI_TARGETS];
    uint32_t ui_target_count;
    /* The eight bit render resolution intermediates between the tonemap and the interface
       composite, each confirmed by a draw that reads the composite and writes it. An empty set
       promotes the composite alone, which is what was measured as cleaner but not sharper. */
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

/* Fill in the substitution plan for `rsf_frame_tap_set_plan`.

   Separate from prepare because the two belong to different modules: this one owns the textures,
   the tap owns the hooks, and the plan is the only thing that has to cross between them. The
   caller sets the plan's gate callback and depth policy; this fills the substitutions and the
   viewport scale. */
rsf_promote_result rsf_promote_fill_plan(rsf_promote* promote, rsf_frame_tap_plan* plan);

rsf_promote_result rsf_promote_get_status(rsf_promote* promote, rsf_promote_status* status);

void rsf_promote_destroy(rsf_promote* promote);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_SCENE_PROMOTE_H */
