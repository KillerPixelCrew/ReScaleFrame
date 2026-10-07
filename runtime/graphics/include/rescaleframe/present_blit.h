/* SPDX-License-Identifier: GPL-3.0-only */
/* Diagnostic fullscreen draw over the finished D3D11 back buffer, before forwarding Present.
   Optional Reinhard/gamma conversion makes linear scene colour visible; it does not reproduce
   the game's grading or HUD. Drawing supports differing source/back-buffer formats that
   CopyResource cannot convert. Use scene promotion for reinsertion into the game's pipeline. */

#ifndef RSF_PRESENT_BLIT_H
#define RSF_PRESENT_BLIT_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_PRESENT_BLIT_ABI_VERSION 1u

typedef int32_t rsf_present_blit_result;
#define RSF_PRESENT_BLIT_OK ((rsf_present_blit_result)0)
#define RSF_PRESENT_BLIT_ERROR_INVALID_ARGUMENT ((rsf_present_blit_result)-1)
#define RSF_PRESENT_BLIT_ERROR_ABI_MISMATCH ((rsf_present_blit_result)-2)
#define RSF_PRESENT_BLIT_ERROR_SHADER_FAILED ((rsf_present_blit_result)-3)
#define RSF_PRESENT_BLIT_ERROR_RESOURCE_FAILED ((rsf_present_blit_result)-4)
/* The swap chain would not hand over its back buffer, which is not something a caller can fix. */
#define RSF_PRESENT_BLIT_ERROR_NO_BACK_BUFFER ((rsf_present_blit_result)-5)

typedef void (*rsf_present_blit_log_fn)(void* user, const char* message);

typedef struct rsf_present_blit_setup {
    uint32_t struct_size;
    uint32_t abi_version;
    rsf_present_blit_log_fn log;
    void* log_user;
} rsf_present_blit_setup;

typedef struct rsf_present_blit rsf_present_blit;

/* `d3d11_device` is the game's `ID3D11Device*`, retained until destroy. */
rsf_present_blit_result rsf_present_blit_create(void* d3d11_device,
                                                const rsf_present_blit_setup* setup,
                                                rsf_present_blit** out);

/* Draw `source`, an `ID3D11Texture2D*`, over the swap chain's back buffer.

   `context` is the immediate context and `swapchain` an `IDXGISwapChain*`. Call from inside a
   Present hook, before forwarding: after the game has finished the frame and before it is shown.

   `tonemap` applies a rough Reinhard curve and a gamma, which is what makes linear scene colour
   look like a picture rather than a dark one. It is for looking at, not for output.

   Saves/restores OM targets/blend/depth, VS/PS, IA layout/topology, rasterizer/viewports, and PS
   resource/sampler/constant slot 0. It omits shader linkage and OM UAVs and inherits GS/HS/DS,
   predication, and stream output; the owner supplies a compatible scope and same-device resources.
   Calls and destruction must be serialized with render work. */
rsf_present_blit_result rsf_present_blit_draw(rsf_present_blit* blit, void* context,
                                              void* swapchain, void* source, uint32_t tonemap);

/* Release owned device/pipeline objects; null is accepted. */
void rsf_present_blit_destroy(rsf_present_blit* blit);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_PRESENT_BLIT_H */
