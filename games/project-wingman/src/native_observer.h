// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>

namespace rsf::wingman {
rsf_result prepare(const rsf_game_prepare_args*) noexcept;
rsf_result start(const rsf_game_start_args*) noexcept;
rsf_result quiesce(const rsf_game_control_args*) noexcept;
rsf_result stop(const rsf_game_control_args*) noexcept;
rsf_result status(rsf_game_renderer_status*) noexcept;
}
