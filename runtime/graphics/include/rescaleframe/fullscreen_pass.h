/* SPDX-License-Identifier: GPL-3.0-only */
/* Draw one texture over another, with the blend the job needs and nothing else.

   Three things in this runtime want the same triangle: the debug view that puts a reconstruction on
   the screen, the composite that puts the interface back over a HUD-less frame, and the overlay's
   own background. They differ in where they draw, what blend they use and whether they tonemap, and
   in nothing else, so they are one pass with a mode rather than three copies of a pipeline.

   The composite is why the blend matters. Every frame generation SDK this project targets asks for
   the interface premultiplied and composited as

       final = ui.rgb + (1 - ui.a) * scene.rgb

   which is `ONE / INV_SRC_ALPHA` with the layer's own alpha, and is identical across Streamline,
   FidelityFX and XeFG. Getting it from the same code that draws the debug view means the picture
   the vendor composites and the picture we composite cannot drift apart.

   Every piece of pipeline state this touches is saved and restored, scissor rectangles included.
   The game is between its own draws and did not ask for its bindings to change; a state left
   altered here is a rendering fault somewhere else entirely, which is the hardest kind to trace. */

#ifndef RSF_FULLSCREEN_PASS_H
#define RSF_FULLSCREEN_PASS_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_FULLSCREEN_PASS_ABI_VERSION 1u

typedef int32_t rsf_fullscreen_result;
#define RSF_FULLSCREEN_OK ((rsf_fullscreen_result)0)
#define RSF_FULLSCREEN_ERROR_INVALID_ARGUMENT ((rsf_fullscreen_result)-1)
#define RSF_FULLSCREEN_ERROR_ABI_MISMATCH ((rsf_fullscreen_result)-2)
#define RSF_FULLSCREEN_ERROR_SHADER_FAILED ((rsf_fullscreen_result)-3)
#define RSF_FULLSCREEN_ERROR_RESOURCE_FAILED ((rsf_fullscreen_result)-4)

/* What the pass does with the source. */
typedef uint32_t rsf_fullscreen_mode;
/* Replace the target with the source, opaque. The plain blit. */
#define RSF_FULLSCREEN_COPY ((rsf_fullscreen_mode)0)
/* Replace the target, with a rough Reinhard curve and a gamma first. Linear scene colour is nearly
   black without it. For looking at, never for output: it is not the game's grade. */
#define RSF_FULLSCREEN_TONEMAP ((rsf_fullscreen_mode)1)
/* Composite a premultiplied layer over what is already there: `ui.rgb + (1 - ui.a) * dst`. The one
   mode with a blend, and the one the vendor contract names. */
#define RSF_FULLSCREEN_PREMULTIPLIED ((rsf_fullscreen_mode)2)
/* The source's alpha as a grey picture, opaque. This is how a layer that looks empty is told from
   one that is empty: a layer with no alpha composites to nothing and looks identical to a broken
   one, and this frame has spent runs on exactly that kind of ambiguity. */
#define RSF_FULLSCREEN_ALPHA ((rsf_fullscreen_mode)3)

typedef void (*rsf_fullscreen_log_fn)(void* user, const char* message);

typedef struct rsf_fullscreen_setup {
    uint32_t struct_size;
    uint32_t abi_version;
    rsf_fullscreen_log_fn log;
    void* log_user;
} rsf_fullscreen_setup;

typedef struct rsf_fullscreen_pass rsf_fullscreen_pass;

/* `d3d11_device` is the game's `ID3D11Device*`, retained until destroy. */
rsf_fullscreen_result rsf_fullscreen_pass_create(void* d3d11_device,
                                                 const rsf_fullscreen_setup* setup,
                                                 rsf_fullscreen_pass** out);

typedef struct rsf_fullscreen_draw {
    uint32_t struct_size;
    rsf_fullscreen_mode mode;
    /* Multiplies the colour before the tonemap curve. Ignored in every other mode. */
    float exposure;
    /* The viewport to draw under, in pixels. Zero means the target's full extent, which is what a
       composite wants; a caller scaling into part of a target says so here. */
    uint32_t width;
    uint32_t height;
} rsf_fullscreen_draw;

/* Draw `source` into `target`.

   `context` is the immediate context, `target` an `ID3D11RenderTargetView*` and `source` an
   `ID3D11ShaderResourceView*`. No depth stencil view is ever bound: this is an overlay, and binding
   one would also mean matching its extent to the target's, which is a constraint the caller should
   not inherit from a blit.

   Taking views rather than textures is deliberate. The caller already has them, creating one per
   draw would allocate on the frame's hottest path, and a view is also where the format
   reinterpretation lives that a typeless render target needs. */
rsf_fullscreen_result rsf_fullscreen_pass_draw(rsf_fullscreen_pass* pass, void* context,
                                               void* target, void* source,
                                               const rsf_fullscreen_draw* parameters);

void rsf_fullscreen_pass_destroy(rsf_fullscreen_pass* pass);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_FULLSCREEN_PASS_H */
