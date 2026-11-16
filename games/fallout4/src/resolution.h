// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_renderer.h>
namespace rsf::fallout4 {
bool verify_resolution() noexcept;
bool native_resolution_inputs(const rsf_game_render_pass&) noexcept;
// Successful reconstruction alone admits the unjittered output/UI scope.
bool begin_output_resolution() noexcept;
void end_output_resolution() noexcept;
bool resolution_busy() noexcept;
}
