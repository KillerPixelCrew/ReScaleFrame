// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_api.h>
namespace rsf::skyrim {
bool presentation_inputs_guard();
bool presentation_inputs_start(const rsf_game_host_services&);
void presentation_inputs_quiesce();
bool presentation_inputs_stop();
bool presentation_inputs_busy();
// Copy identity only. Textures and contexts remain borrowed by the native callbacks.
void identify_presentation_inputs(const rsf_game_render_pass&);
}
