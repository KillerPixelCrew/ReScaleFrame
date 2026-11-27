// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/overlay.h>
#ifdef __cplusplus
extern "C" {
#endif
int rsf_overlay_d3d12_frame(void* device, void* queue, void* swapchain, const rsf_overlay_stats*, rsf_overlay_intent*,
    void (*log)(void*,const char*), void* user);
void rsf_overlay_d3d12_stop();
int rsf_overlay_native12_present(void*, void*, void*, const rsf_overlay_stats*, rsf_overlay_intent*,
    void (*)(void*,const char*), void*);
#ifdef __cplusplus
}
#endif
