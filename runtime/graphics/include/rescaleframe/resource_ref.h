/* SPDX-License-Identifier: GPL-3.0-only */
/* Hold on to a D3D11 resource from code that has no COM.

   The loader is C and the resources it is handed are borrowed for the duration of a callback. When
   something has to outlive that callback, the borrow has to become a reference, and taking one
   means calling through a vtable that C cannot name. Hence these two.

   They are deliberately the whole of it. Anything more would be a COM wrapper, and the point of the
   C ABI line is that the C side does not acquire one. */

#ifndef RSF_RESOURCE_REF_H
#define RSF_RESOURCE_REF_H

#ifdef __cplusplus
extern "C" {
#endif

/* Take a reference on an `IUnknown*`, which every D3D11 interface is. Null is accepted and does
   nothing, so a caller need not test first. */
void rsf_resource_retain(void* resource);

/* Drop one. Releasing a null does nothing, and releasing more than was taken is the caller's
   mistake to avoid: nothing here counts on its behalf. */
void rsf_resource_release(void* resource);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_RESOURCE_REF_H */
