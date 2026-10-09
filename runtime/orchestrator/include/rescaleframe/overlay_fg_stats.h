/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_OVERLAY_FG_STATS_H
#define RSF_OVERLAY_FG_STATS_H
/* The frame-generation half of rsf_overlay_stats, filled the same way for the D3D11 proxy
   (dlss_bridge.c, C) and the Unity D3D12 host (unity_sr_host.cpp). Header-only and plain C so both
   include it; the callers link the runtime that provides the choice and present-bridge queries.

   The caller owns everything else in `stats`, including display_refresh_mhz (only the D3D11
   proxy knows the display). `fg` is the status from whichever FG path the caller drives and may be
   NULL, which leaves the status fields zero. */
#include <rescaleframe/overlay.h>
#include <rescaleframe/fg_choice.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/d3d11_present_bridge.h>

static inline void rsf_overlay_fill_fg_stats(rsf_overlay_stats* stats, const rsf_native_fg_status* fg)
{
    const rsf_fg_choice choice = rsf_fg_choice_get();
    stats->fg_backend = rsf_d3d11_present_has_owner() ? rsf_d3d11_present_backend() : 0;
    stats->fg_requested_backend = choice.backend;
    stats->fg_backend_choices = choice.choices | RSF_OVERLAY_FG_RUNTIME_SWITCH;
    stats->fg_selection_result = choice.last_result;
    if (!stats->fg_selection_result) stats->fg_selection_result = rsf_d3d11_present_switch_result();
    if (!fg) return;
    stats->fg_available = fg->available;
    stats->fg_requested_mode = fg->requested.mode;
    stats->fg_requested_generated = fg->requested.generated_frames;
    stats->fg_effective_mode = fg->vendor.effective_mode;
    stats->fg_effective_generated = fg->vendor.effective_generated_frames;
    stats->fg_active = fg->vendor.active;
    stats->fg_max_generated = fg->vendor.max_generated_frames;
    stats->reflex_available = fg->vendor.low_latency_available;
    stats->reflex_requested_mode = fg->requested.reflex_mode;
    stats->reflex_effective_mode = fg->vendor.effective_reflex;
    stats->fg_reason = fg->reason;
    stats->fg_last_result = fg->last_result;
    stats->fg_total_presented = fg->vendor.total_presented;
    stats->fg_present_count_valid = fg->available && (fg->vendor.valid_statistics & RSF_FG_STAT_TOTAL_PRESENTED) != 0;
    stats->frame_limit_us = fg->requested.frame_limit_us;
}
#endif
