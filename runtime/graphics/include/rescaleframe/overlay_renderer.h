/* SPDX-License-Identifier: GPL-3.0-only */
/* Draw the overlay's triangles with D3D11, inside a frame the game owns.

   `rescaleframe/overlay.h` is the whole description of what to draw: vertices in physical pixels
   with premultiplied colour, indices, per draw call scissor rectangles, and a texture atlas that
   arrives in patches. This is the other half, the part that has to happen on the game's device
   context between two of its own draws.

   Everything difficult here is a consequence of that placement. The context arrives configured for
   whatever the game was doing, so every piece of state this pass sets is read back first and put
   back afterwards. Anything missed does not break the overlay, it breaks the game's rendering after
   the overlay returns, which is a bug that reads as the game being broken.

   The renderer never creates a render target and never rebinds one. It draws into whatever is bound
   when it is called, because the caller knows which image the overlay belongs on and this does not.
   Rebinding would mean calling OMSetRenderTargets, which also unbinds every unordered access view
   the output merger holds, and those cannot be put back exactly.

   Colour is written through unchanged: egui produces premultiplied sRGB encoded bytes, and a game's
   back buffer is normally a UNORM format holding sRGB encoded values, so a pass through is a match.
   That is a choice, not a measurement. Against an _SRGB render target view the hardware would
   encode a second time and the overlay would look washed out; the draw notices that case and says
   so through the log sink once, rather than applying a conversion nobody has been able to look
   at. */

#ifndef RSF_OVERLAY_RENDERER_H
#define RSF_OVERLAY_RENDERER_H

#include <stdint.h>

#include <rescaleframe/overlay.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_OVERLAY_RENDERER_ABI_VERSION 1u

typedef int32_t rsf_overlay_renderer_result;
#define RSF_OVERLAY_RENDERER_OK ((rsf_overlay_renderer_result)0)
#define RSF_OVERLAY_RENDERER_ERROR_INVALID_ARGUMENT ((rsf_overlay_renderer_result)-1)
#define RSF_OVERLAY_RENDERER_ERROR_ABI_MISMATCH ((rsf_overlay_renderer_result)-2)
/* No HLSL compiler could be loaded, or a shader did not compile. */
#define RSF_OVERLAY_RENDERER_ERROR_SHADER_FAILED ((rsf_overlay_renderer_result)-3)
/* A device resource could not be created, mapped or updated. */
#define RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED ((rsf_overlay_renderer_result)-4)

/* Progress and diagnostics, the same shape and the same reason as the rest of this directory: this
   runs on a game's render thread, where a returned code often reaches nobody. */
typedef void (*rsf_overlay_renderer_log_fn)(void* user, const char* message);

typedef struct rsf_overlay_renderer_setup {
    uint32_t struct_size;
    uint32_t abi_version;
    rsf_overlay_renderer_log_fn log;
    void* log_user;
} rsf_overlay_renderer_setup;

typedef struct rsf_overlay_renderer rsf_overlay_renderer;

/* Build the renderer against one device. `d3d11_device` is an `ID3D11Device*`, kept referenced
   until the renderer is destroyed.

   Shaders are compiled here, at load, rather than shipped as bytecode. The cross build has no
   shader compiler, so a pass built from bytecode would exist only in the MSVC build and could not
   be built or looked at anywhere else. */
rsf_overlay_renderer_result rsf_overlay_renderer_create(void* d3d11_device,
                                                        const rsf_overlay_renderer_setup* setup,
                                                        rsf_overlay_renderer** out);

/* Apply one atlas change from `rsf_overlay_texture_updates`. `context` is an
   `ID3D11DeviceContext*`.

   A whole texture update creates or recreates the destination at that size. A patch writes a
   sub-rectangle of a texture that already exists, and one for an unknown id or outside the
   destination is rejected rather than guessed at. Both cases have to work: egui sends the font
   atlas whole once and then patches it every time a glyph is used for the first time, so a
   renderer that only handles whole updates looks correct until it suddenly does not.

   Expects an immediate context. Patches go through `UpdateSubresource` with a destination box,
   which D3D11 documents as behaving wrongly on a deferred context. */
rsf_overlay_renderer_result rsf_overlay_renderer_upload_texture(
    rsf_overlay_renderer* renderer, void* context, const rsf_overlay_texture_update* update);

/* Release a texture the overlay has finished with. An unknown id is not an error: the overlay may
   report the same id as freed after it has already been dropped. */
void rsf_overlay_renderer_free_texture(rsf_overlay_renderer* renderer, uint64_t id);

/* Draw one frame's worth of draw calls into the currently bound render target. `context` is an
   `ID3D11DeviceContext*`, and `target_width` and `target_height` are the size in physical pixels of
   the image the overlay was laid out for. The vertex shader turns pixels into clip space with
   those, so no projection matrix has to be plumbed through.

   The draw data belongs to the overlay and only until its next frame, so it is consumed here and
   not remembered. Draw calls with an empty scissor rectangle, and any naming a texture that was
   never uploaded, are skipped rather than drawn wrong.

   Pipeline state is saved before and restored after: input assembler, every shader stage that takes
   part in a draw, constant, vertex and index buffers, blend state with its factor and mask, depth
   stencil state with its reference, rasterizer state, viewports, scissor rectangles, and the shader
   resource and sampler slots this uses. Render targets are the one exception, and are not touched
   at all for the reason above. */
rsf_overlay_renderer_result rsf_overlay_renderer_draw(rsf_overlay_renderer* renderer, void* context,
                                                      const rsf_overlay_draw_data* draw_data,
                                                      uint32_t target_width,
                                                      uint32_t target_height);

void rsf_overlay_renderer_destroy(rsf_overlay_renderer* renderer);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_OVERLAY_RENDERER_H */
