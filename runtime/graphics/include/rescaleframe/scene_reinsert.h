/* SPDX-License-Identifier: GPL-3.0-only */
/* Put a reconstructed scene back into the game's own frame, so its grade and its interface survive.

   The debug view in present_blit.h draws the reconstruction over the finished frame. That is how
   the result got looked at at all, and it is not how this should work: the image it shows is
   pre-tonemap scene colour, so it is ungraded, and everything the game composited afterwards is
   gone with it. The interface included.

   What the frame actually does, at a reduced render scale and from a replayed capture in
   docs/research/ac7-frame-capture.md:

     scene colour (render res, HDR)
       -> post chain, tonemap and grade   -> composite (render res)
       -> the interface's own target       -> composited into the same composite
       -> one draw                         -> back buffer (output res)

   So the last draw is a spatial upscale of a picture that already has the interface in it, which is
   why raising the render scale sharpens the interface too. The scene has to be reconstructed before
   the grade, and the interface has to be drawn after it at output resolution, or the reconstruction
   buys a sharp scene and a soft HUD.

   The whole intervention is three substitutions and a scale:

     - the composite becomes an output resolution texture of the same format, so everything drawn
       into it lands at output resolution;
     - the interface's target becomes one too, so the interface is drawn at output resolution rather
       than magnified from render resolution;
     - the scene colour the tonemap reads becomes the reconstruction, from the moment the composite
       is first bound in a frame and no earlier, because the scene passes read scene colour as well
       and handing them a reconstruction of the frame they are still drawing is a feedback loop;
     - viewports and scissor rectangles are scaled while a substituted target is bound, since the
       game asks for the render resolution it believes it is drawing at.

   The last draw then reads an output resolution composite and writes an output resolution back
   buffer, so the game's own upscale becomes a copy. Nothing is removed from the frame.

   What this does not fix, and it is worth being plain about. The bloom chain is still computed from
   the render resolution scene, so what the tonemap composites over the reconstruction is a low
   resolution glow: acceptable because bloom is low frequency, and not the same as correct. Post
   process shaders that do texel addressed work rather than normalised sampling will address the
   wrong texels, because their constants still describe the buffer the engine believes it has. Both
   are visible only in a rendered result, and neither has been looked at.

   Nothing here is game specific. It is given textures and sizes; which textures those are is the
   caller's question, and in this repository the loader answers it by watching the frame's tail. */

#ifndef RSF_SCENE_REINSERT_H
#define RSF_SCENE_REINSERT_H

#include <rescaleframe/frame_tap.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_REINSERT_ABI_VERSION 2u
/* How many surfaces the interface may be composited into. Four is two frames' worth of the pair AC7
   alternates between, which leaves room for the count to have been miscounted without silently
   dropping one. */
#define RSF_REINSERT_MAX_INTERFACE_TARGETS 4u

typedef int32_t rsf_reinsert_result;
#define RSF_REINSERT_OK ((rsf_reinsert_result)0)
#define RSF_REINSERT_ERROR_INVALID_ARGUMENT ((rsf_reinsert_result)-1)
#define RSF_REINSERT_ERROR_ABI_MISMATCH ((rsf_reinsert_result)-2)
/* A replacement texture or one of its views could not be created. */
#define RSF_REINSERT_ERROR_RESOURCE_FAILED ((rsf_reinsert_result)-3)
/* The tail describes a frame that is already at output resolution, so there is nothing to reinsert
   into: substituting here would replace a texture with one of its own size and change nothing
   except the number of textures in the frame. */
#define RSF_REINSERT_ERROR_NOT_SCALED ((rsf_reinsert_result)-4)

typedef void (*rsf_reinsert_log_fn)(void* user, const char* message);

typedef struct rsf_reinsert_setup {
    uint32_t struct_size;
    uint32_t abi_version;
    /* The game's `ID3D11Device*`, retained until destroy. Its own device, because the replacements
       have to be usable in the game's own draws. */
    void* device;
    /* Presented resolution, which is what everything here is promoted to. */
    uint32_t output_width;
    uint32_t output_height;
    rsf_reinsert_log_fn log;
    void* log_user;
} rsf_reinsert_setup;

/* The part of the frame this substitutes into, as the caller found it. Every texture is an
   `ID3D11Texture2D*` belonging to the game, borrowed for the call and not retained: this reads
   their descriptors and keeps their addresses, so a caller has to keep them alive itself for as
   long as the plan is in use. */
typedef struct rsf_reinsert_frame_tail {
    uint32_t struct_size;
    /* The render resolution target the interface is composited into and the last draw reads. */
    void* composite;
    /* The targets the interface is composited into. An empty set is allowed and means none was
       identified: the scene is still reconstructed and the interface is magnified with it, which is
       better than substituting a texture that might be something else.

       A set rather than one, because the game uses more than one. AC7 rasterizes its interface at a
       fixed 1920x1080 and then composites it down into a render resolution surface, and the surface
       it picks alternates between at least two allocations from frame to frame. Promoting one of
       them leaves the other frames squashed, which looks like the promotion not working at all
       rather than like it working half the time.

       This is the fourth time a rule of the form "the one that matches" has been wrong in this
       frame. Composite selection, interface format, translucent layer identity and now this. */
    void* interface_targets[RSF_REINSERT_MAX_INTERFACE_TARGETS];
    uint32_t interface_target_count;
    /* The scene colour the tonemap reads, and what replaces it. */
    void* scene_color;
    /* The reconstruction, at output resolution. `rsf_dlss_pipeline_output_texture` is one. */
    void* reconstruction;
    /* What the game is rendering at, which is what viewports have to be scaled from. */
    uint32_t render_width;
    uint32_t render_height;
} rsf_reinsert_frame_tail;

typedef struct rsf_reinsert_status {
    uint32_t struct_size;
    /* Whether a tail has been prepared and the plan is fit to hand to the frame tap. */
    uint32_t ready;
    /* Whether the interface is being drawn at output resolution, or magnified with the scene. */
    uint32_t interface_promoted;
    uint32_t render_width;
    uint32_t render_height;
    uint32_t output_width;
    uint32_t output_height;
} rsf_reinsert_status;

typedef struct rsf_reinsert rsf_reinsert;

rsf_reinsert_result rsf_reinsert_create(const rsf_reinsert_setup* setup, rsf_reinsert** out);

/* Build the replacements for this tail. Safe to call again with a different tail, which is what a
   render scale change or a swap chain resize needs; the previous replacements are released.

   Call it from the thread that drives the frames. It creates textures and views, which is not
   something to do from a key polling thread while the render thread is drawing. */
rsf_reinsert_result rsf_reinsert_prepare(rsf_reinsert* reinsert,
                                         const rsf_reinsert_frame_tail* tail);

/* Fill in the substitution plan for `rsf_frame_tap_set_plan`.

   Separate from prepare because the two belong to different modules: this one owns the textures,
   the tap owns the hooks, and the plan is the only thing that has to cross between them. */
rsf_reinsert_result rsf_reinsert_fill_plan(rsf_reinsert* reinsert, rsf_frame_tap_plan* plan);

rsf_reinsert_result rsf_reinsert_get_status(rsf_reinsert* reinsert, rsf_reinsert_status* status);

void rsf_reinsert_destroy(rsf_reinsert* reinsert);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_SCENE_REINSERT_H */
