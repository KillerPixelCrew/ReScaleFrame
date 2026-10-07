/* SPDX-License-Identifier: GPL-3.0-only */
/* Fixed-capacity cache of replacement D3D11 constant buffers, keyed by original buffer address.
   The caller supplies the replacement contents and invalidates identities when buffers are
   refilled or recycled. Writes use Map(WRITE_DISCARD); serialize all calls on the render thread.
   Twin buffers allocate lazily and are reused when the least recently written entry is evicted. */

#ifndef RSF_CONSTANT_TWIN_H
#define RSF_CONSTANT_TWIN_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_CONSTANT_TWINS 32u

typedef struct rsf_constant_twins rsf_constant_twins;

/* Retain ID3D11Device. bytes must be nonzero and a multiple of 16; each twin has that width.
   Null on invalid arguments/allocation failure. Originals are not retained. */
rsf_constant_twins* rsf_constant_twins_create(void* device, uint32_t bytes);
void rsf_constant_twins_destroy(rsf_constant_twins* twins);

/* Fill the twin of `original` with `contents`, creating it the first time. Returns the twin's
   `ID3D11Buffer*`, borrowed until its slot is rewritten/reused or the cache is destroyed. contents
   contains at least the configured byte width. context belongs to device; null on create/map failure. */
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
