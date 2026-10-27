// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/overlay.h>
int rsf_overlay_d3d12_frame(void* device, void* queue, void* swapchain, const rsf_overlay_stats*, rsf_overlay_intent*,
    void (*log)(void*,const char*), void* user);
void rsf_overlay_d3d12_stop();
uint32_t rsf_overlay_native12_visible();
void rsf_overlay_native12_toggle();
int rsf_overlay_native12_present(void*, void*, void*, const rsf_overlay_stats*, rsf_overlay_intent*,
    void (*)(void*,const char*), void*);
/* Completed game backbuffer is PRESENT on entry and remains PRESENT on return. */
int rsf_overlay_d3d12_present(void* device, void* queue, void* swapchain, const rsf_overlay_stats*, rsf_overlay_intent*,
    void (*log)(void*,const char*), void* user);
