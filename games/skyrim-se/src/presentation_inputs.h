// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
namespace rsf::skyrim {
bool presentation_inputs_guard();
bool presentation_inputs_start(const rsf_game_host_services&);
void presentation_inputs_quiesce();
bool presentation_inputs_stop();
bool presentation_inputs_busy();
// Report native pre-HDR inputs before SR or post-processing modifies them.
bool prepare_presentation_inputs(void* manager, const rsf_game_host_services&);
void finish_presentation_inputs();
}
