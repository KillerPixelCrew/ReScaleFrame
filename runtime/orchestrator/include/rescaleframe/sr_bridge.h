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
/* Join earlier bridge work before transactional selection; refusal preserves the active backend.
   A GPU fault latches the bridge unusable for further selection/evaluation. */
RSF_RUNTIME_API rsf_backend_result rsf_sr_bridge_select(rsf_sr_bridge* bridge,
    rsf_sr_backend backend, rsf_quality quality, uint64_t version_id);
/* Exposure mode is a context-creation flag. Drain outstanding work before replacing the context;
   enabled is 0 or 1 and a refusal preserves the previous policy. */
RSF_RUNTIME_API rsf_backend_result rsf_sr_bridge_set_auto_exposure(rsf_sr_bridge* bridge,
                                                                  uint32_t enabled);
/* Frame resources are D3D11 textures. Prepared depth must be R32_FLOAT and motion must contain
   dense, previous-minus-current displacement. No sparse sentinel is accepted. Transfers only
   the declared origin-zero render rectangle. Output is a caller-owned UAV-capable color texture.
   Run on the D3D11 immediate-context owner thread, with game bindings saved by the caller.
   Success queues the completed output copy on that context; later draws/readbacks on the same
   context are ordered after it. It does not imply CPU-observed GPU completion. */
RSF_RUNTIME_API rsf_backend_result rsf_sr_bridge_evaluate(rsf_sr_bridge* bridge,
    void* d3d11_context, const rsf_sr_frame* frame);
/* Read only on the bridge owner thread into size-initialized caller storage. */
RSF_RUNTIME_API rsf_backend_result rsf_sr_bridge_get_status(const rsf_sr_bridge* bridge,
    rsf_sr_session_status* status);
/* Null is allowed. Wait up to ten seconds for bridge completion; if it times out while the device
   remains live, resources are intentionally retained until process exit for pending GPU users. */
RSF_RUNTIME_API void rsf_sr_bridge_destroy(rsf_sr_bridge* bridge);
#ifdef __cplusplus
}
#endif
#endif
