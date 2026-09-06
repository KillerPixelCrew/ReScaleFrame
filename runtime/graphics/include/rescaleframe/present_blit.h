/* SPDX-License-Identifier: GPL-3.0-only */
/* Put a texture on the screen, over whatever the game was about to present.

   This exists because a reconstruction that only writes into a texture cannot be judged. Counters
   say it ran and a dumped frame says the geometry is right, but the faults that matter most in an
   upscaler are temporal: ghosting, smearing behind a moving object, a motion vector whose sign is
   inverted. None of those are visible in a still image, and all of them are obvious in a second of
   movement. So the result has to reach the screen.

   It is a debug view and says so. The image handed over here is scene colour from partway through
   the frame: linear, not tonemapped, and with no interface composited onto it. Drawing it over the
   finished frame therefore replaces a graded image with an ungraded one, which will look wrong in
   brightness and lack a HUD even when the reconstruction itself is perfect. An optional rough
   tonemap makes it viewable. Neither the blit nor that tonemap is a step towards how this should
   eventually work: the real path reinserts the reconstructed scene before the game's own
   composite, and this is a way to look at the result in the meantime.

   A copy would be simpler and does not work. The result is `R16G16B16A16_FLOAT` and a swap chain
   here is `R10G10B10A2_UNORM`, and `CopyResource` requires matching formats, so this is a draw. */

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

   Every piece of pipeline state this touches is saved and restored. The game is between its own
   draws and did not ask for its bindings to change. */
rsf_present_blit_result rsf_present_blit_draw(rsf_present_blit* blit, void* context,
                                              void* swapchain, void* source, uint32_t tonemap);

void rsf_present_blit_destroy(rsf_present_blit* blit);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_PRESENT_BLIT_H */
