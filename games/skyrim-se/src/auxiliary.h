// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
namespace rsf::skyrim {
void observe_auxiliaries(void* context, const rsf_game_render_pass&, const rsf_game_host_services&);
bool reconstruct_auxiliaries(void* context, const rsf_game_render_pass&, const rsf_game_host_services&);
void finish_auxiliaries(const rsf_game_host_services&);
bool auxiliaries_pending();
}
