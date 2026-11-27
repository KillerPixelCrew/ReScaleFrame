/* SPDX-License-Identifier: MIT */
#include <rescaleframe/d3d11_host.h>
#include <stddef.h>
_Static_assert(sizeof(rsf_d3d11_reduced_frame) == 56, "Recovery request size");
_Static_assert(offsetof(rsf_d3d11_reduced_frame, token) == 16, "Token offset");
_Static_assert(offsetof(rsf_d3d11_reduced_frame, owner) == 24, "Owner offset");
_Static_assert(offsetof(rsf_d3d11_reduced_frame, surfaces) == 40, "Surfaces offset");
_Static_assert(offsetof(rsf_d3d11_reduced_frame, temporal_result) == 48, "Result offset");
_Static_assert(_Alignof(rsf_d3d11_reduced_frame) == 8, "Recovery request alignment");
_Static_assert(sizeof(rsf_d3d11_chain_dispatch) == 72, "Chain request size");
_Static_assert(offsetof(rsf_d3d11_chain_dispatch, receiver) == 16, "Receiver offset");
_Static_assert(offsetof(rsf_d3d11_chain_dispatch, args) == 24, "Arguments offset");
_Static_assert(offsetof(rsf_d3d11_chain_dispatch, result) == 64, "Chain result offset");
_Static_assert(sizeof(rsf_d3d11_capture_control) == 16, "Capture control size");

int rsf_d3d11_host_fixture(void);

int main(void)
{
    rsf_frame_record rendered = {0};
    rendered.struct_size = (uint32_t)sizeof(rendered);
    rendered.source_provenance = RSF_FRAME_SOURCE_RENDERED;
    if (rsf_frame_has_rendered_source(&rendered)) return 1;
    rendered.source_qpc = 10;
    rendered.source_qpc_frequency = 1000;
    if (!rsf_frame_has_rendered_source(&rendered)) return 1;
    rendered.input_qpc = 10;
    if (rsf_frame_has_rendered_source(&rendered)) return 1;

    rsf_frame_record gameplay = {0};
    gameplay.struct_size = (uint32_t)sizeof(gameplay);
    gameplay.screen = RSF_SCREEN_GAMEPLAY;
    if (!rsf_frame_allows_fg(&gameplay)) return 1;
    gameplay.flags = RSF_FRAME_FLAG_NO_FG;
    if (rsf_frame_allows_fg(&gameplay)) return 1;
    return rsf_d3d11_host_fixture();
}
