// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/overlay.h>
#ifdef __cplusplus
extern "C" {
#endif
int rsf_overlay_d3d12_frame(void* device, void* queue, void* swapchain, const rsf_overlay_stats*, rsf_overlay_intent*,
    void (*log)(void*,const char*), void* user);
void rsf_overlay_d3d12_stop();
/* Draw on the application backbuffer in PRESENT, with input on a separate owner window.
   Returns 1 for a draw, 0 for a hidden panel and -1 if initialization failed. */
int rsf_overlay_d3d12_frame_for_window(void* device, void* queue, void* backbuffer, void* input_window,
    const rsf_overlay_stats*, rsf_overlay_intent*, void (*log)(void*, const char*), void* user);
uint32_t rsf_overlay_native12_visible();
void rsf_overlay_native12_toggle();
int rsf_overlay_native12_present(void*, void*, void*, const rsf_overlay_stats*, rsf_overlay_intent*,
    void (*)(void*,const char*), void*);
/* Completed game backbuffer is PRESENT on entry and remains PRESENT on return. */
int rsf_overlay_d3d12_present(void* device, void* queue, void* swapchain, const rsf_overlay_stats*, rsf_overlay_intent*,
    void (*log)(void*,const char*), void* user);
#ifdef __cplusplus
}
#endif
