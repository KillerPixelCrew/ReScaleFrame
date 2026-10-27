// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_api.h>
bool rsf_rdr2_scene_prepare(const void* image, const rsf_game_host_services&) noexcept;
bool rsf_rdr2_scene_start() noexcept;
bool rsf_rdr2_scene_quiesce() noexcept;
void rsf_rdr2_scene_stop() noexcept;
void rsf_rdr2_scene_upscale(void* commands, const rsf_game_render_pass&) noexcept;
void rsf_rdr2_scene_swapchain(void* swapchain) noexcept;
uint64_t rsf_rdr2_scene_source(uint64_t native_frame) noexcept;
