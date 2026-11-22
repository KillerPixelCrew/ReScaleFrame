/* SPDX-License-Identifier: MIT */
#include <rescaleframe/game_renderer.h>

/* The runtime and native backend consumers share the SDK admission predicates. */
int rsf_core_frame_allows_fg(const rsf_frame_record* frame)
{
    return rsf_frame_allows_fg(frame);
}

int rsf_core_frame_has_rendered_source(const rsf_frame_record* frame)
{
    return rsf_frame_has_rendered_source(frame);
}

uint32_t rsf_core_depth_domains(const rsf_game_render_pass* pass)
{
    return rsf_game_depth_domains_requested(&pass->depth_domains);
}
