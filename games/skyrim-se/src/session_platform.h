// SPDX-License-Identifier: MIT
#pragma once
#include <cstddef>
#include <rescaleframe/d3d11_host.h>

static_assert(sizeof(rsf_motion_decode_params) == 48);
static_assert(offsetof(rsf_d3d11_host_setup, depth_far_plane) == 160 && offsetof(rsf_d3d11_host_setup, motion) == 80 &&
    offsetof(rsf_d3d11_host_setup, quality_choices) == 128 &&
    offsetof(rsf_d3d11_host_setup, accept_window) == 136 &&
    offsetof(rsf_d3d11_host_setup, colour_transport_default) == 156);

extern "C" bool rsf_skyrim_session_accepting() noexcept;
// Copies the services into out: while accepting when live, otherwise as retained. Returns the
// Runtime rsf_d3d11_host_bind_presentation export, or 0.
extern "C" uintptr_t rsf_skyrim_session_lease(bool live, rsf_game_host_services* out) noexcept;
extern "C" void rsf_skyrim_session_withdraw() noexcept;
