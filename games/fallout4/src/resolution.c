/* SPDX-License-Identifier: MIT */
/* Guarded Runtime calls and the reduced-stage receipt text of the Rust resolution owner
   (resolution.rs). */
#include "resolution_platform.h"
#include "native_guard.h"
#include <stdio.h>

bool rsf_fallout4_resolution_reconfigure(rsf_game_process_upscale_fn upscale, void* user, void* context,
    const rsf_game_render_pass* planning)
{
    bool returned = true;
    RSF_FALLOUT4_TRY { (void)upscale(user, context, planning); }
    RSF_FALLOUT4_CATCH_CXX { returned = false; }
    return returned;
}

int32_t rsf_fallout4_resolution_config(rsf_game_render_config_fn query, void* user, rsf_game_render_config* config)
{
    int32_t answer = 0;
    RSF_FALLOUT4_TRY { answer = query(user, config) != 0 ? 1 : 0; }
    RSF_FALLOUT4_CATCH_CXX { answer = -1; }
    return answer;
}

void rsf_fallout4_resolution_report_receipt(ResolutionReport report, const rsf_game_render_pass* pass, void* context,
    const NativeResolutionReceipt* receipt)
{
    const NativeResolutionReceipt* facts = receipt;
    char text[1700] = {0};
    snprintf(text, sizeof(text),
        "Fallout4 reduced-stage=%s session=%llu source=%llu native=%llu/%u result=%d result_is_evaluation=%u published=%u scope=%u/%u changed=%u scalar_read=%u owner=%u input_ratio=%g,%g current_ratio=%g,%g viewport=%u active=%u partial=%u geometry=%p,%p,%p shadow_read=%u shadow=%g,%g,%g,%g,%g,%g dirty=0x%x rs_count=%u rs_read=%u rs=%g,%g,%g,%g,%g,%g camera=%ux%u->%ux%u context=%p",
        facts->stage, (unsigned long long)pass->session_id,
        (unsigned long long)pass->source_frame_id, (unsigned long long)pass->native_frame, facts->frame,
        facts->result, (unsigned)facts->evaluation, (unsigned)facts->published, (unsigned)facts->input,
        (unsigned)facts->output, (unsigned)facts->changed, (unsigned)facts->scalars, (unsigned)facts->owner,
        (double)facts->input_ratio[0], (double)facts->input_ratio[1], (double)facts->ratio[0],
        (double)facts->ratio[1], (unsigned)facts->viewport, (unsigned)facts->active, (unsigned)facts->partial,
        (void*)facts->geometry[0], (void*)facts->geometry[1], (void*)facts->geometry[2], (unsigned)facts->shadow,
        (double)facts->raster[0], (double)facts->raster[1], (double)facts->raster[2], (double)facts->raster[3],
        (double)facts->raster[4], (double)facts->raster[5], facts->dirty, facts->actual_count, facts->actual_read,
        (double)facts->actual_raster[0], (double)facts->actual_raster[1], (double)facts->actual_raster[2],
        (double)facts->actual_raster[3], (double)facts->actual_raster[4], (double)facts->actual_raster[5],
        pass->camera.render_width, pass->camera.render_height, pass->camera.output_width,
        pass->camera.output_height, context);
    rsf_fallout4_report_guarded(report, text);
}
