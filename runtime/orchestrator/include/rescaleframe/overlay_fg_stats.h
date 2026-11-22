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
void rsf_core_overlay_fill_fg_stats(rsf_overlay_stats* stats, const rsf_native_fg_status* fg,
    int (*latency_status_get)(rsf_native_latency_status*));
#ifdef __cplusplus
}
#endif

static inline void rsf_overlay_fill_fg_stats(rsf_overlay_stats* stats, const rsf_native_fg_status* fg)
{
    rsf_core_overlay_fill_fg_stats(stats, fg, rsf_native_latency_status_get);
}
#endif
