/* SPDX-License-Identifier: MIT */
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
   without history. Sizes must match what the backend will be evaluated at.
   When the dispatch was submitted but its completion was not observed (a timed-out wait or a failed
   queue signal) and the device was not removed, the first error is returned and the dispatch stays
   pending with its resources and the SDK context alive. While it is pending, warm_up, select
   (including RSF_SR_NONE), a set_auto_exposure change and evaluate return
   RSF_BACKEND_ERROR_NOT_READY, and close returns NOT_READY without consuming the bridge. Each of those calls
   first checks the dispatch's own fence, re-signalled behind it on the bridge queue if the first
   signal failed, or device removal; evaluate only polls, the others wait up to 10 s. Once either is
   observed the resources are released and the bridge is usable again. */
RSF_RUNTIME_API rsf_backend_result rsf_sr_bridge_warm_up(rsf_sr_bridge* bridge, uint32_t render_width,
    uint32_t render_height, uint32_t output_width, uint32_t output_height, uint32_t manual_exposure);
RSF_RUNTIME_API rsf_backend_result rsf_sr_bridge_get_status(const rsf_sr_bridge* bridge,
    rsf_sr_session_status* status);
/* OK consumes the bridge. NOT_READY leaves it owned by the caller, which may retry
   close on the immediate-context owner thread. Null returns INVALID_ARGUMENT. */
RSF_RUNTIME_API rsf_backend_result rsf_sr_bridge_close(rsf_sr_bridge* bridge);
/* Compatibility entry point for callers that cannot retry retirement. */
RSF_RUNTIME_API void rsf_sr_bridge_destroy(rsf_sr_bridge* bridge);
#ifdef __cplusplus
}
#endif
#endif
