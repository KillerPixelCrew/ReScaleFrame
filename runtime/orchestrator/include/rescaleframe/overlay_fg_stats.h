/* SPDX-License-Identifier: MIT */
#ifndef RSF_OVERLAY_FG_STATS_H
#define RSF_OVERLAY_FG_STATS_H
/* The frame-generation half of rsf_overlay_stats, filled the same way for the D3D12 hosts; the
   linked Runtime owns the mapping.

   The caller owns everything else in `stats`. `fg` is the status from whichever FG path the
   caller drives and may be NULL, which leaves the status fields zero. */
#include <rescaleframe/overlay.h>
#include <rescaleframe/fg_choice.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/d3d11_present_bridge.h>

#ifdef __cplusplus
extern "C" {
#endif
void rsf_overlay_fill_fg_stats(rsf_overlay_stats* stats, const rsf_native_fg_status* fg);

#ifdef __cplusplus
}
#endif

#endif
