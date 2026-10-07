/* SPDX-License-Identifier: GPL-3.0-only */
/* Synchronous D3D11 buffer readback through a temporary staging buffer. Copies the whole GPU
   buffer and returns its leading bytes. The blocking Map waits for GPU work; callers control
   sampling frequency and serialize calls on the immediate context's owning thread. */

#ifndef RSF_CONSTANT_BUFFER_READ_H
#define RSF_CONSTANT_BUFFER_READ_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_CONSTANT_BUFFER_READ_ABI_VERSION 1u

typedef int32_t rsf_constant_buffer_result;
#define RSF_CONSTANT_BUFFER_OK ((rsf_constant_buffer_result)0)
/* A null argument, a zero length, or a resource that is not a buffer. */
#define RSF_CONSTANT_BUFFER_ERROR_INVALID_ARGUMENT ((rsf_constant_buffer_result)-1)
/* Reserved for a future versioned call structure; current entry points do not return it. */
#define RSF_CONSTANT_BUFFER_ERROR_ABI_MISMATCH ((rsf_constant_buffer_result)-2)
/* The buffer holds fewer bytes than were asked for. */
#define RSF_CONSTANT_BUFFER_ERROR_TOO_SMALL ((rsf_constant_buffer_result)-3)
/* The staging buffer could not be created. */
#define RSF_CONSTANT_BUFFER_ERROR_STAGING_FAILED ((rsf_constant_buffer_result)-4)
/* The staging copy could not be mapped. */
#define RSF_CONSTANT_BUFFER_ERROR_MAP_FAILED ((rsf_constant_buffer_result)-5)
/* Buffer/context belong to a different device. Checked before issuing the GPU copy. */
#define RSF_CONSTANT_BUFFER_ERROR_FOREIGN_DEVICE ((rsf_constant_buffer_result)-6)

/* Copy the first `bytes` of `buffer` into `destination`.

   `device` is an `ID3D11Device*`, `context` an `ID3D11DeviceContext*` and `buffer` an
   `ID3D11Buffer*`. `destination` is caller owned and must hold at least `bytes`. Buffer and context
   must both belong to `device`, which is checked rather than assumed.

   The context has to be the immediate one. A deferred context cannot map for reading, and this maps
   without the do-not-wait flag, so the call blocks until the copy has run. Nothing about the
   game's own state is touched; the copy reads a resource it is free to keep using. */
rsf_constant_buffer_result rsf_read_constant_buffer(void* device, void* context, void* buffer,
                                                    void* destination, uint32_t bytes);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_CONSTANT_BUFFER_READ_H */
