/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <rescaleframe/d3d11_host.h>

_Static_assert(sizeof(rsf_motion_decode_params) == 48, "motion decode layout");
_Static_assert(offsetof(rsf_d3d11_host_setup, depth_far_plane) == 160 && offsetof(rsf_d3d11_host_setup, motion) == 80 &&
    offsetof(rsf_d3d11_host_setup, quality_choices) == 128 &&
    offsetof(rsf_d3d11_host_setup, accept_window) == 136 &&
    offsetof(rsf_d3d11_host_setup, colour_transport_default) == 156, "d3d11 host setup layout");

bool rsf_skyrim_session_accepting(void);
/* Copies the services into out: while accepting when live, otherwise as retained. Returns the
   Runtime rsf_d3d11_host_bind_presentation export, or 0. */
uintptr_t rsf_skyrim_session_lease(bool live, rsf_game_host_services* out);
void rsf_skyrim_session_withdraw(void);
