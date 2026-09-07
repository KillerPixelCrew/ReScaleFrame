/* SPDX-License-Identifier: GPL-3.0-only */
/* One texture, visible to both a D3D11 device and a D3D12 one.
 *
 * Every frame generation SDK this project targets is D3D12 only. AC7 is D3D11. So the frame the
 * game draws has to reach a device it was not created on, and this is how: create on D3D11 with a
 * shared NT handle, open the same memory on D3D12, and synchronise with a fence both can see.
 *
 * The legacy `D3D11_RESOURCE_MISC_SHARED` flag is deliberately not used. It produces a handle only
 * another D3D11 device can open, which is the one thing that would not help.
 *
 * Whether this works at all outside Windows is the open question this file exists to answer.
 * DXVK has to export a handle that vkd3d-proton can open, and the two are separate translations of
 * separate APIs onto Vulkan with no obligation to agree about memory. The fixture reports the
 * `HRESULT`s and skips rather than failing when they disagree, because that is a fact about the
 * environment and not a defect in this code; the run under Proton is the real measurement.
 */

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
/* A handle was produced and the D3D12 device would not open it. This is the interesting failure,
   and the one the fixture reports rather than hides: it means the two runtimes disagree about
   shared memory, which is a property of where this is running. */
#define RSF_SHARED_ERROR_OPEN_FAILED ((rsf_shared_result)-5)

typedef void (*rsf_shared_log_fn)(void* user, const char* message);

typedef struct rsf_shared_surface_setup {
    uint32_t struct_size;
    uint32_t abi_version;
    uint32_t width;
    uint32_t height;
    /* A DXGI_FORMAT. Typeless formats are refused: a shared surface is opened by another runtime
       that has no way to ask what was intended, so the format has to mean something on both sides. */
    uint32_t format;
    /* Non-zero to also bind as a render target on the D3D11 side, which the layer and the HUD-less
       copy both need. */
    uint32_t render_target;
    rsf_shared_log_fn log;
    void* log_user;
} rsf_shared_surface_setup;

typedef struct rsf_shared_surface rsf_shared_surface;

/* Create on D3D11 and open on D3D12.
 *
 * `d3d11_device` must be an `ID3D11Device*` and `d3d12_device` an `ID3D12Device*` on the same
 * adapter; sharing across adapters is not a thing D3D12 will do and is checked by the caller, not
 * here. A null `d3d12_device` creates the D3D11 side and the handle only, which is what a caller
 * wants when it is testing whether sharing is available before committing to a bridge. */
rsf_shared_result rsf_shared_surface_create(void* d3d11_device, void* d3d12_device,
                                            const rsf_shared_surface_setup* setup,
                                            rsf_shared_surface** out);

void rsf_shared_surface_destroy(rsf_shared_surface* surface);

/* The D3D11 texture, as an `ID3D11Texture2D*`. Borrowed. */
void* rsf_shared_surface_d3d11(rsf_shared_surface* surface);
/* The same memory as an `ID3D12Resource*`, or null when no D3D12 device was given. Borrowed. */
void* rsf_shared_surface_d3d12(rsf_shared_surface* surface);
/* The D3D11 render target view, or null when one was not asked for. Borrowed. */
void* rsf_shared_surface_target(rsf_shared_surface* surface);

/* A fence both devices can wait on, created on D3D11 and opened on D3D12.
 *
 * Separate from the surfaces because one fence orders every surface in a frame: a fence per surface
 * would mean a wait per surface, and the whole set is handed over at one moment anyway. */
typedef struct rsf_shared_fence rsf_shared_fence;

rsf_shared_result rsf_shared_fence_create(void* d3d11_device, void* d3d12_device,
                                          rsf_shared_log_fn log, void* log_user,
                                          rsf_shared_fence** out);
void rsf_shared_fence_destroy(rsf_shared_fence* fence);

/* The `ID3D11Fence*` and the `ID3D12Fence*` onto the same object. Borrowed. */
void* rsf_shared_fence_d3d11(rsf_shared_fence* fence);
void* rsf_shared_fence_d3d12(rsf_shared_fence* fence);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_SHARED_SURFACE_H */
