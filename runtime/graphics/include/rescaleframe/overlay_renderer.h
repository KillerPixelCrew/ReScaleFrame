/* SPDX-License-Identifier: GPL-3.0-only */
/* Render borrowed overlay meshes and atlas patches into the currently bound D3D11 target.
   Vertices use physical pixels and premultiplied encoded colour. UNORM target views match that
   encoding; an sRGB target view encodes it again and is reported once. No conversion is inferred.
   The renderer owns device, shaders, geometry buffers, and uploaded textures. Serialize upload,
   draw, free, and destroy on the device's immediate-context thread. It restores affected bindings
   and never rebinds render targets or OM UAVs. Callers retain responsibility for other inherited
   pipeline effects such as predication and stream output. */

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

/* Optional synchronous diagnostic sink; message storage lasts only for the call. */
typedef void (*rsf_overlay_renderer_log_fn)(void* user, const char* message);

typedef struct rsf_overlay_renderer_setup {
    uint32_t struct_size;
    uint32_t abi_version;
    rsf_overlay_renderer_log_fn log;
    void* log_user;
} rsf_overlay_renderer_setup;

typedef struct rsf_overlay_renderer rsf_overlay_renderer;

/* Retain ID3D11Device and compile shaders at creation using the system d3dcompiler_47.dll.
   Successful creation transfers an owned renderer through out; release with destroy. */
rsf_overlay_renderer_result rsf_overlay_renderer_create(void* d3d11_device,
                                                        const rsf_overlay_renderer_setup* setup,
                                                        rsf_overlay_renderer** out);

/* Borrow and upload one RGBA8 atlas update. Whole updates create/replace an id atomically after
   resource creation succeeds; patches require an existing id and bounds within its texture.
   context is the same-device immediate context; pixel storage lasts for this call only. */
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

/* Release device, pipeline, geometry, and all remaining atlas entries; null is accepted. */
void rsf_overlay_renderer_destroy(rsf_overlay_renderer* renderer);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_OVERLAY_RENDERER_H */
