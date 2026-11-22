/* SPDX-License-Identifier: MIT */
/* Take and drop D3D11 resource references from code that has no COM.

   The loader is C and a resource it is handed is borrowed for the duration of a callback. These
   two are deliberately the whole of it: anything more would be a COM wrapper, and the point of the
   C ABI line is that the C side does not acquire one. */

#ifndef RSF_RESOURCE_REF_H
#define RSF_RESOURCE_REF_H

#ifdef __cplusplus
extern "C" {
#endif

/* The texture behind a swap chain, as an `ID3D11Texture2D*` with a reference the caller releases.

   `swapchain` is an `IDXGISwapChain*`; buffer 0 is the one a frame ends in. Null on failure, which
   is a swap chain that is not backed by a D3D11 texture and not something a caller can fix. */
void* rsf_swapchain_back_buffer(void* swapchain);

/* Drop one reference on an `IUnknown*`. Releasing a null does nothing, and releasing more than was
   taken is the caller's mistake to avoid: nothing here counts on its behalf. */
void rsf_resource_release(void* resource);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_RESOURCE_REF_H */
