// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/overlay.h>
/* Graphics/presentation owner only. Draw into the current D3D12 backbuffer through D3D11On12,
   borrowing device/queue/chain for this call and retaining the initialized interop device.
   Caller supplies RENDER_TARGET state on entry/exit and owns the surrounding GPU ordering.
   Return the overlay draw result; zero includes initialization refusal. Log/user outlive stop. */
int rsf_overlay_d3d12_frame(void* device, void* queue, void* swapchain, const rsf_overlay_stats*, rsf_overlay_intent*,
    void (*log)(void*,const char*), void* user);
/* Stop overlay users first and complete their queue work before releasing device resources. */
void rsf_overlay_d3d12_stop();
