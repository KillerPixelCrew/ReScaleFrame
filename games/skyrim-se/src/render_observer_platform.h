// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
#include <stddef.h>

extern "C" void rsf_skyrim_render_end_enter(rsf_game_host_services*) noexcept;
extern "C" void rsf_skyrim_render_end_leave(const rsf_game_host_services*) noexcept;
struct rsf_skyrim_upload_observation {
    uint64_t session;
    uint32_t frame;
};
static_assert(sizeof(rsf_skyrim_upload_observation) == 16 && offsetof(rsf_skyrim_upload_observation, frame) == 8);
extern "C" bool rsf_skyrim_render_upload_enter(uintptr_t renderer,
    rsf_skyrim_upload_observation*) noexcept;
extern "C" void rsf_skyrim_render_upload_leave(const rsf_skyrim_upload_observation*, uint8_t flags) noexcept;
extern "C" void rsf_skyrim_render_jitter_observed(uintptr_t state) noexcept;
extern "C" void rsf_skyrim_reset_render_observations() noexcept;
extern "C" bool rsf_skyrim_render_accept_window(void* window) noexcept;
