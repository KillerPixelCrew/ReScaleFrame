// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_api.h>
rsf_result rsf_gta5_renderer_prepare(const rsf_game_host_services&) noexcept;
rsf_result rsf_gta5_renderer_start() noexcept;
rsf_result rsf_gta5_renderer_quiesce() noexcept;
rsf_result rsf_gta5_renderer_stop() noexcept;
void rsf_gta5_renderer_status(rsf_game_renderer_status&) noexcept;
