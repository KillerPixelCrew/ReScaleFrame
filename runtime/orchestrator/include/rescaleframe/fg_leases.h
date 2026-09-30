/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_FG_LEASES_H
#define RSF_FG_LEASES_H
#include <rescaleframe/frame_generation.h>
#include <rescaleframe/runtime.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_fg_leases rsf_fg_leases;
/* Graphics-owner-thread operations. Acquire retains up to five ID3D12Resource inputs before
   prepare/tagging; submit retains the queue submission fence. Seal follows application Present
   and its provider state query. A null vendor fence means vendor reads are already retired.
   Both fences must finish before collect releases resources or permits slot reuse. */
RSF_RUNTIME_API rsf_backend_result rsf_fg_leases_create(uint32_t capacity, rsf_fg_leases**);
RSF_RUNTIME_API rsf_backend_result rsf_fg_leases_acquire(rsf_fg_leases*, uint64_t frame_id,
    uint32_t resource_generation, void* const* resources, uint32_t count);
RSF_RUNTIME_API rsf_backend_result rsf_fg_leases_submit(rsf_fg_leases*, uint64_t frame_id,
    void* d3d12_fence, uint64_t value);
RSF_RUNTIME_API rsf_backend_result rsf_fg_leases_seal(rsf_fg_leases*, uint64_t frame_id,
    const rsf_fg_retirement*);
RSF_RUNTIME_API rsf_backend_result rsf_fg_leases_collect(rsf_fg_leases*, uint32_t* pending);
/* Cancel only before submitting commands to a queue. Submitted failures must still seal and retire. */
RSF_RUNTIME_API rsf_backend_result rsf_fg_leases_cancel(rsf_fg_leases*, uint64_t frame_id);
/* A refusal retains the live handle and all COM references. */
RSF_RUNTIME_API rsf_backend_result rsf_fg_leases_destroy(rsf_fg_leases*);
#ifdef __cplusplus
}
#endif
#endif
