/* SPDX-License-Identifier: MIT */
#ifndef RSF_OVERLAY_FG_STATS_H
#define RSF_OVERLAY_FG_STATS_H
/* The frame-generation half of rsf_overlay_stats, filled the same way for the D3D11 proxy
   (dlss_bridge.c, C) and the generic D3D12 hosts. Plain C so both
   include it; the linked Runtime owns the mapping.

   The caller owns everything else in `stats`, including display_refresh_mhz (only the D3D11
   proxy knows the display). `fg` is the status from whichever FG path the caller drives and may be
   NULL, which leaves the status fields zero. */
#include <rescaleframe/overlay.h>
#include <rescaleframe/fg_choice.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/d3d11_present_bridge.h>

#ifdef __cplusplus
extern "C" {
#endif
void rsf_overlay_fill_fg_stats(rsf_overlay_stats* stats, const rsf_native_fg_status* fg);
/* Request the provider a panel choice names on the presenting `chain`. Auto becomes the provider
   this GPU prefers among those the bridge serves, so the caller needs no policy of its own. Returns
   an rsf_backend_result; Auto with no provider to pick requests nothing and returns zero. */
int rsf_overlay_request_fg_backend(void* chain, uint32_t backend);
#ifdef __cplusplus
}
#endif

#endif
