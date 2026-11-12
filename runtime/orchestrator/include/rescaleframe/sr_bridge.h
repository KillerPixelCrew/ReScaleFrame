/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_SR_BRIDGE_H
#define RSF_SR_BRIDGE_H
#include <rescaleframe/sr_session.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_sr_bridge rsf_sr_bridge;
/* setup.open.device is the game's D3D11 device. Creates D3D12 on that same adapter.
   GPU fences order SR transfers without presentation or swap-chain ownership. CPU waits are
   limited to command allocator reuse and resource/backend retirement. */
RSF_RUNTIME_API rsf_backend_result rsf_sr_bridge_create(const rsf_sr_session_setup* setup,
                                                        rsf_sr_bridge** out);
RSF_RUNTIME_API rsf_backend_result rsf_sr_bridge_select(rsf_sr_bridge* bridge,
    rsf_sr_backend backend, rsf_quality quality, uint64_t version_id);
/* Frame resources are D3D11 textures. Prepared depth must be R32_FLOAT and motion must contain
   dense, previous-minus-current displacement. No sparse sentinel is accepted. Transfers only
   the declared origin-zero render rectangle. Output is a caller-owned UAV-capable color texture.
   Run on the D3D11 immediate-context owner thread, with game bindings saved by the caller.
   Success queues the completed output copy on that context; later draws/readbacks on the same
   context are ordered after it. It does not imply CPU-observed GPU completion. */
RSF_RUNTIME_API rsf_backend_result rsf_sr_bridge_set_auto_exposure(rsf_sr_bridge* bridge,
                                                                  uint32_t enabled);
RSF_RUNTIME_API rsf_backend_result rsf_sr_bridge_evaluate(rsf_sr_bridge* bridge,
    void* d3d11_context, const rsf_sr_frame* frame);
/* Build the active backend's lazily created pipelines now: one throwaway dispatch on the bridge's own
   D3D12 queue with blank resources of the given sizes (the formats the evaluate path uses), waited
   for on the CPU. FSR4 builds its pipelines at its first dispatch, which is 25 s on a cold driver
   cache. Needs no D3D11 immediate context. `manual_exposure` also supplies a 1x1 exposure texture. The next evaluate starts
   without history. Sizes must match what the backend will be evaluated at. */
RSF_RUNTIME_API rsf_backend_result rsf_sr_bridge_warm_up(rsf_sr_bridge* bridge, uint32_t render_width,
    uint32_t render_height, uint32_t output_width, uint32_t output_height, uint32_t manual_exposure);
RSF_RUNTIME_API rsf_backend_result rsf_sr_bridge_get_status(const rsf_sr_bridge* bridge,
    rsf_sr_session_status* status);
RSF_RUNTIME_API void rsf_sr_bridge_destroy(rsf_sr_bridge* bridge);
#ifdef __cplusplus
}
#endif
#endif
