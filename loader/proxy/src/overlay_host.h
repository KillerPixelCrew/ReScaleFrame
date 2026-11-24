/* SPDX-License-Identifier: MIT */
/* Shared D3D11 overlay host C ABI. The Rust orchestrator owns the panel, renderer and input
   capture for both the swap-chain and supplied-target paths. */

#ifndef RSF_PROXY_OVERLAY_HOST_H
#define RSF_PROXY_OVERLAY_HOST_H

#include <rescaleframe/overlay.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Announced rather than returned, for the same reason as the rest of this directory: a code
   returned on a game's render thread often reaches nobody. */
typedef void (*rsf_overlay_host_log_fn)(void* user, const char* message);

/* Load the panel, build its renderer against the game's device, and subclass the window the swap
   chain was created against.

   `swapchain` is an `IDXGISwapChain*`; its device owns all overlay resources. Returns non-zero when the
   overlay is ready to be drawn. Safe to call again once it has succeeded, which does nothing.

   The panel DLL is found through `RSF_OVERLAY_DLL` when that names a file, and otherwise beside
   this module. Failing to find it is a reported refusal and not a crash, because a research build
   without the Rust half is a normal thing to be running. */
int rsf_overlay_host_start(void* swapchain, rsf_overlay_host_log_fn log,
                           void* log_user);

/* Whether the panel is currently open. */
unsigned int rsf_overlay_host_visible(void);

/* Lay out and draw one frame, from inside the Present hook and on the render thread.

   The immediate context is acquired from the presenting chain's device. `stats` is what the panel
   displays, filled by the caller because the numbers live in the bridge. `intent` is what the
   panel wants changed, written when this returns non-zero; it is a description of what the user
   clicked and nothing here acts on it.

   Returns zero when both panel and startup hint are hidden, has not started, or could not draw. Drawing binds the
   back buffer, because the renderer deliberately never rebinds a render target, and puts back the
   targets that were bound afterwards. */
int rsf_overlay_host_present(void* swapchain, const rsf_overlay_stats* stats,
                             rsf_overlay_intent* intent);

/* Give up the window subclass, the renderer and the panel. Uninstalling a window procedure cannot
   make a message already inside it finish first, so this carries the same race every hook in this
   tree does and is meant for a point where the message thread is known to be elsewhere. */
void rsf_overlay_host_stop(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_PROXY_OVERLAY_HOST_H */
