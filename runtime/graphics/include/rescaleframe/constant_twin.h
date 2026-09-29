/* SPDX-License-Identifier: GPL-3.0-only */
/* A constant buffer of our own that stands in for one of the game's, with contents the caller
   chose, for binding in its place on particular draws.

   Written for the interface: AC7 draws its panels with a view's uniform buffer, and a view the
   reconstruction jitters gives those panels a projection that wobbles where nothing resolves it.
   Binding a twin whose contents are that view with the jitter taken out draws the panel exactly
   where an unjittered engine would have. What goes into the twin is the caller's business; this
   only keeps a twin per original, fills it, and finds it again.

   Fixed capacity, the least recently written twin reused when full, so nothing allocates on the
   render thread after the first frames. The twins are dynamic buffers filled with
   Map(WRITE_DISCARD) on the context the caller passes, which must be the one the game draws on. */

#ifndef RSF_CONSTANT_TWIN_H
#define RSF_CONSTANT_TWIN_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_CONSTANT_TWINS 32u

typedef struct rsf_constant_twins rsf_constant_twins;

/* `device` is the game's `ID3D11Device*`, retained. Every twin is `bytes` wide. Null on failure. */
rsf_constant_twins* rsf_constant_twins_create(void* device, uint32_t bytes);
void rsf_constant_twins_destroy(rsf_constant_twins* twins);

/* Fill the twin of `original` with `contents`, creating it the first time. Returns the twin's
   `ID3D11Buffer*`, borrowed, or null when it could not be created or filled. */
void* rsf_constant_twins_write(rsf_constant_twins* twins, void* context, void* original,
                               const void* contents);

/* The twin of `original`, or null. Checks the bound buffer's descriptor to reject an address
   recycled for a differently sized resource. `original` must be a live ID3D11Buffer. */
void* rsf_constant_twins_find(const rsf_constant_twins* twins, const void* original);

/* Forget `original`, which the game has refilled with something that needs no twin. */
void rsf_constant_twins_forget(rsf_constant_twins* twins, const void* original);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_CONSTANT_TWIN_H */
