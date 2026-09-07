/* SPDX-License-Identifier: GPL-3.0-only */
/* Put the egui overlay on the screen inside the game, and give what it decides back to the bridge.

   The three pieces this joins already existed and had never been connected to anything: the panel
   itself is a Rust DLL behind `rescaleframe/overlay.h`, the D3D11 pass that draws its triangles is
   `overlay_renderer`, and the window subclass that feeds it a mouse and a keyboard is
   `overlay_input`. What was missing was a caller inside a game process, which is this.

   It lives in the research proxy for the same reason the rest of the bridge does: this is what can
   be loaded into Ace Combat 7 today. Presentation belongs to the orchestrator by the ownership
   split in AGENTS.md, and moving it there is its own change.

   The point of it is not the settings. Counters can say a reconstruction ran, and a dumped frame
   can say its geometry is right, but whether the picture is actually correct is a question that
   has to be looked at while the game moves. That is what this is for. */

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

/* Open or close it from a hotkey, rather than from the key the input module watches for. */
void rsf_overlay_host_toggle(void);

/* Lay out and draw one frame, from inside the Present hook and on the render thread.

   The immediate context is acquired from the presenting chain's device. `stats` is what the panel
   displays, filled by the caller because the numbers live in the bridge. `intent` is what the
   panel wants changed, written when this returns non-zero; it is a description of what the user
   clicked and nothing here acts on it.

   Returns zero when the overlay is closed, has not started, or could not draw. Drawing binds the
   back buffer, because the renderer deliberately never rebinds a render target, and puts back the
   targets that were bound the way `present_blit` does at the same point in the frame. */
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
