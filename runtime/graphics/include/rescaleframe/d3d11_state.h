/* SPDX-License-Identifier: GPL-3.0-only */
/* Preserve immediate-context bindings around work inside a game's frame. Engines cache bindings
   and will not necessarily rebind them after a vendor call. All six shader stages, their complete
   resource ranges, shader linkage, constant-buffer ranges, IA, rasterizer, OM, UAV and predication
   bindings are retained. Stream-output targets resume their existing write cursors.

   This preserves bindings, not resource contents or asynchronous query execution. Wrapped work
   must not write native append/consume or stream-output buffers. Deferred contexts are refused.
   The opaque storage owns a private allocation until restore; it must not be copied. */

#ifndef RSF_D3D11_STATE_H
#define RSF_D3D11_STATE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Fixed C storage. The complete snapshot is private and does not change this layout. */
#define RSF_D3D11_STATE_BYTES 8192

typedef struct rsf_d3d11_state {
    /* Aligned for pointers, which is what almost all of this holds. */
    uint64_t opaque[RSF_D3D11_STATE_BYTES / 8];
} rsf_d3d11_state;

/* Read the whole pipeline into `state`. `context` is an `ID3D11DeviceContext*`.

   Every interface pointer taken here carries a reference, so a save must be matched by exactly one
   restore. Use fresh storage and restore on the same context/thread before reuse. Returns non-zero
   when the state was taken;
   zero means invalid arguments, a deferred context, or allocation failure. Nothing was saved. */
uint32_t rsf_d3d11_state_save(void* context, rsf_d3d11_state* state);

/* Put it all back and drop the references. Safe to call only on a state a save filled in.

   Conflicting inputs and outputs are cleared before the saved bindings are restored. Hidden UAV
   counters are preserved, not reset. */
void rsf_d3d11_state_restore(void* context, rsf_d3d11_state* state);

/* Narrow scope for immediate depth replay. Saves only OM targets/depth state and the pixel
   shader (including linkage). Caller must refuse OM UAVs before entering this scope. No input,
   rasterizer, predicate, or stream-output binding is touched. Use fresh storage and pair with the
   matching depth restore on the same context; do not mix full/depth snapshot entry points. */
uint32_t rsf_d3d11_depth_state_save(void* context, rsf_d3d11_state* state);
void rsf_d3d11_depth_state_restore(void* context, rsf_d3d11_state* state);

/* Two small device calls for a C caller. `rsf_d3d11_copy_resource` is CopyResource on `context`
   (`ID3D11DeviceContext*`) from `source` to `destination`, both `ID3D11Resource*` of one size and
   format. `rsf_d3d11_create_shader_view` makes a default shader resource view on `texture`
   (`ID3D11Texture2D*`) with `device` (`ID3D11Device*`), returned as `ID3D11ShaderResourceView*`
   with one reference the caller releases, or null. */
void rsf_d3d11_copy_resource(void* context, void* destination, void* source);
void* rsf_d3d11_create_shader_view(void* device, void* texture);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_D3D11_STATE_H */
