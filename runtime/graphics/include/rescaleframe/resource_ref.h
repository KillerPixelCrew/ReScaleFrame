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

/* The texture behind a swap chain, as an `ID3D11Texture2D*` with a reference the caller releases.

   `swapchain` is an `IDXGISwapChain*`; buffer 0 is the one a frame ends in. Null on failure, which
   is a swap chain that is not backed by a D3D11 texture and not something a caller can fix.

   Here for the same reason as the two above: naming the back buffer is how C code says which
   render target it means, and asking for it means two calls through vtables C cannot spell. */
void* rsf_swapchain_back_buffer(void* swapchain);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_RESOURCE_REF_H */
