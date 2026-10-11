// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_api.h>

namespace rsf::wingman {
inline constexpr char known_sha256[] =
    "092e84225624a4de9c05d2404ff55269bd4a2aa2ff0548f2a183c6bf36abc85a";
rsf_result prepare(const rsf_game_prepare_args*) noexcept;
rsf_result start(const rsf_game_start_args*) noexcept;
rsf_result quiesce(const rsf_game_control_args*) noexcept;
rsf_result stop(const rsf_game_control_args*) noexcept;
rsf_result status(rsf_game_renderer_status*) noexcept;
}
