/* SPDX-License-Identifier: GPL-3.0-only */
/* Read an `ID3D11Buffer` back into system memory, inside the frame that is using it.

   The camera data no backend can substitute for lives in one constant buffer the game fills per
   frame, and a D3D11 constant buffer created the ordinary way is not CPU readable. The only route
   to its contents is a staging copy, so that is what this does: create a staging buffer, copy the
   whole resource into it, map it, and hand back the leading bytes the caller asked for.

   This is deliberately a small helper rather than a cached reader. The copy plus a blocking map
   synchronises with the GPU, which is expensive and belongs to whoever decides how often a frame
   can afford it, not to the read itself. */

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
/* Reserved. No entry point here takes a versioned struct yet, so nothing returns this today. It is
   numbered now so that adding one later does not renumber the codes below, which a caller compiled
   against an older header would still be comparing against. */
#define RSF_CONSTANT_BUFFER_ERROR_ABI_MISMATCH ((rsf_constant_buffer_result)-2)
/* The buffer holds fewer bytes than were asked for. */
#define RSF_CONSTANT_BUFFER_ERROR_TOO_SMALL ((rsf_constant_buffer_result)-3)
/* The staging buffer could not be created. */
#define RSF_CONSTANT_BUFFER_ERROR_STAGING_FAILED ((rsf_constant_buffer_result)-4)
/* The staging copy could not be mapped. */
#define RSF_CONSTANT_BUFFER_ERROR_MAP_FAILED ((rsf_constant_buffer_result)-5)
/* The buffer or the context belongs to a different device than the one passed. Copying across
   devices is invalid D3D11 that faults rather than failing a call, so both are checked before the
   copy and reported separately from an ordinary bad argument: a process with more than one device
   is normal here, and this is the mistake worth seeing by name. */
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
