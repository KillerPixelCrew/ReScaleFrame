/* SPDX-License-Identifier: GPL-3.0-only */
/* C ABI helpers for COM ownership and D3D11 back-buffer views. Retain/release operate on live
   IUnknown interfaces; they do not validate arbitrary pointers or track reference balance. */

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

/* GetBuffer(0) as ID3D11Texture2D with one caller-owned reference, or null on failure. Release all
   back-buffer references/views before ResizeBuffers. A non-D3D11 chain cannot supply this type. */
void* rsf_swapchain_back_buffer(void* swapchain);

/* Default-format RTV with one caller-owned reference. device/texture must be live same-device
   ID3D11Device/ID3D11Texture2D; typeless textures need a typed descriptor and return null here. */
void* rsf_create_render_target_view(void* device, void* texture);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_RESOURCE_REF_H */
