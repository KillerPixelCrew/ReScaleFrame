/* SPDX-License-Identifier: GPL-3.0-only */
/* D3D11/D3D12 shared texture and fence creation through NT handles. Devices must use the same
   adapter; the caller verifies this and orders GPU access. Surface creation alone does not make
   simultaneous reads/writes safe. Signal/wait the shared fence when transferring ownership.
   Translation runtimes may refuse creation or handle opening; return codes preserve that stage. */

#ifndef RSF_SHARED_SURFACE_H
#define RSF_SHARED_SURFACE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_SHARED_SURFACE_ABI_VERSION 1u

typedef int32_t rsf_shared_result;
#define RSF_SHARED_OK ((rsf_shared_result)0)
#define RSF_SHARED_ERROR_INVALID_ARGUMENT ((rsf_shared_result)-1)
#define RSF_SHARED_ERROR_ABI_MISMATCH ((rsf_shared_result)-2)
/* The D3D11 texture could not be created with a shareable NT handle. */
#define RSF_SHARED_ERROR_CREATE_FAILED ((rsf_shared_result)-3)
/* Created, but the runtime would not produce an NT handle for it. */
#define RSF_SHARED_ERROR_NO_HANDLE ((rsf_shared_result)-4)
/* D3D12 refused an exported handle; sharing support can differ across translation runtimes. */
#define RSF_SHARED_ERROR_OPEN_FAILED ((rsf_shared_result)-5)

typedef void (*rsf_shared_log_fn)(void* user, const char* message);

typedef struct rsf_shared_surface_setup {
    uint32_t struct_size;
    uint32_t abi_version;
    uint32_t width;
    uint32_t height;
    /* Typed DXGI_FORMAT; typeless formats are refused to require one cross-API interpretation. */
    uint32_t format;
    /* Nonzero also creates a D3D11 render-target binding and view. */
    uint32_t render_target;
    rsf_shared_log_fn log;
    void* log_user;
} rsf_shared_surface_setup;

typedef struct rsf_shared_surface rsf_shared_surface;

/* Create on ID3D11Device and optionally open on a same-adapter ID3D12Device. The caller checks
   adapter identity. Null d3d12_device creates only the D3D11 texture/view and NT handle, allowing
   callers to probe D3D11 export support without opening a D3D12 resource. After argument/ABI
   validation, *out is cleared and remains null on allocation, export, or open failure. */
rsf_shared_result rsf_shared_surface_create(void* d3d11_device, void* d3d12_device,
                                            const rsf_shared_surface_setup* setup,
                                            rsf_shared_surface** out);

/* Release both API resources/views and close the owned NT handle; null is accepted. The caller
   must retire GPU access before destruction. Borrowed accessors below return null for null input. */
void rsf_shared_surface_destroy(rsf_shared_surface* surface);

/* The D3D11 texture, as an `ID3D11Texture2D*`. Borrowed. */
void* rsf_shared_surface_d3d11(rsf_shared_surface* surface);
/* The same memory as an `ID3D12Resource*`, or null when no D3D12 device was given. Borrowed. */
void* rsf_shared_surface_d3d12(rsf_shared_surface* surface);
/* The D3D11 render target view, or null when one was not asked for. Borrowed. */
void* rsf_shared_surface_target(rsf_shared_surface* surface);

/* Shared D3D11.4/D3D12 fence; a single fence can order all surfaces of a submitted frame. */
typedef struct rsf_shared_fence rsf_shared_fence;

/* Requires ID3D11Device5 fence support. A null D3D12 device creates only the D3D11 side/handle.
   Initial value is zero; caller owns monotonically increasing signal values and queue waits. */
rsf_shared_result rsf_shared_fence_create(void* d3d11_device, void* d3d12_device,
                                          rsf_shared_log_fn log, void* log_user,
                                          rsf_shared_fence** out);
/* Retire all submitted waits/signals before releasing the fence and closing its NT handle. */
void rsf_shared_fence_destroy(rsf_shared_fence* fence);

/* The `ID3D11Fence*` and the `ID3D12Fence*` onto the same object. Borrowed. */
void* rsf_shared_fence_d3d11(rsf_shared_fence* fence);
void* rsf_shared_fence_d3d12(rsf_shared_fence* fence);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_SHARED_SURFACE_H */
