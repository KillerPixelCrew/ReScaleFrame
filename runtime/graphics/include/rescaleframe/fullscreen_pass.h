/* SPDX-License-Identifier: GPL-3.0-only */
/* Fullscreen-triangle copy, diagnostic tonemap, coverage view, and premultiplied UI composite.
   The composite uses ui.rgb + (1 - ui.a) * scene.rgb, matching the frame-generation UI contract.
   Inputs are caller-owned views on the same device. Draws save and restore their affected context
   bindings, including scissors; serialize use and destruction on the owning render thread. */

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
/* Visualise source alpha as opaque grey, including coverage that contributes no visible RGB. */
#define RSF_FULLSCREEN_ALPHA ((rsf_fullscreen_mode)3)
/* Encode the layer before premultiplied blending when it stores linear values and the destination
   already stores encoded values. The caller selects the transfer curve: sRGB is piecewise;
   GAMMA22 is a pure power curve. The API cannot infer the game's transfer function. */
#define RSF_FULLSCREEN_PREMULTIPLIED_SRGB ((rsf_fullscreen_mode)4)
#define RSF_FULLSCREEN_PREMULTIPLIED_GAMMA22 ((rsf_fullscreen_mode)5)

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

   Caller-owned views avoid per-draw texture-view allocation and specify typed formats for
   typeless resources. This narrow snapshot does not preserve OM UAVs or shader linkage, and
   does not disable inherited GS/HS/DS, predication, or stream output. Use a compatible fullscreen
   scope or wrap the call in a full state snapshot and configure those effects explicitly. */
rsf_fullscreen_result rsf_fullscreen_pass_draw(rsf_fullscreen_pass* pass, void* context,
                                               void* target, void* source,
                                               const rsf_fullscreen_draw* parameters);

/* Release pass-owned resources; null is accepted. Finish outstanding use before destruction. */
void rsf_fullscreen_pass_destroy(rsf_fullscreen_pass* pass);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_FULLSCREEN_PASS_H */
