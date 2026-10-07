/* SPDX-License-Identifier: GPL-3.0-only */
/* Private shared host for the Rust panel DLL, D3D11 renderer and window input hook.
   Start/draw/stop on a serialized graphics owner thread. Present drawings have an internal
   skip-if-busy guard; the supplied-target path relies on caller serialization. The host retains
   its selected device and panel module; targets/stats are borrowed per draw. */

#ifndef RSF_PROXY_OVERLAY_HOST_H
#define RSF_PROXY_OVERLAY_HOST_H

#include <rescaleframe/overlay.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Optional synchronous log sink. message is borrowed for the call; user/code must outlive
   the active host and input callbacks. */
typedef void (*rsf_overlay_host_log_fn)(void* user, const char* message);

/* Start against the presenting IDXGISwapChain's D3D11 device and window.
   Non-zero means ready; repeated successful calls are idempotent. RSF_OVERLAY_DLL overrides
   the sibling rescaleframe_overlay.dll. A missing/incompatible DLL is reported and returns zero;
   persistent startup failures in this path suppress repeated attempts. */
int rsf_overlay_host_start(void* swapchain, rsf_overlay_host_log_fn log,
                           void* log_user);
/* Start using a caller-supplied ID3D11Device and HWND, retaining the device. draw_target
   borrows an immediate context/RTV and pixel extents belonging to that device; serialize calls
   and provide a point where binding targets is safe. A zero result means hidden, unavailable or
   draw refusal. intent may be null; inspect the result before acting on returned intent. */
int rsf_overlay_host_start_device(void* d3d11_device, void* hwnd, rsf_overlay_host_log_fn log, void* user);
int rsf_overlay_host_draw_target(void* d3d11_context, void* render_target_view, uint32_t width,
    uint32_t height, const rsf_overlay_stats*, rsf_overlay_intent*);

/* Whether the panel is currently open. */
unsigned int rsf_overlay_host_visible(void);

/* Toggle the input module's panel visibility after startup. */
void rsf_overlay_host_toggle(void);

/* Draw during the selected swapchain's Present callback. A temporary backbuffer/RTV is
   acquired and released within the call, preserving ResizeBuffers lifetime. Different-device
   presents and overlapping draws are skipped. stats is borrowed; intent is written on success.
   Return zero when hidden, unstarted or refused. Render targets/depth are restored, but binding
   the overlay target can unbind OM UAVs; callers must use an end-of-frame boundary. */
int rsf_overlay_host_present(void* swapchain, const rsf_overlay_stats* stats,
                             rsf_overlay_intent* intent);

/* Stop at a boundary where drawing and window callbacks have drained. Uninstalling the
   subclass does not wait for an already executing message callback. Release panel/renderer/device
   state; retain the panel DLL for process lifetime so callback code cannot unload underneath it. */
void rsf_overlay_host_stop(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_PROXY_OVERLAY_HOST_H */
