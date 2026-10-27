// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_renderer.h>

namespace rsf::wingman {
bool read_camera(const void* view, rsf_game_render_pass& pass) noexcept;
}
