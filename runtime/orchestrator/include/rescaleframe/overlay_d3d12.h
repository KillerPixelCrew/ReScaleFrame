// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/overlay.h>
int rsf_overlay_d3d12_frame(void* device, void* queue, void* swapchain, const rsf_overlay_stats*, rsf_overlay_intent*,
    void (*log)(void*,const char*), void* user);
void rsf_overlay_d3d12_stop();
