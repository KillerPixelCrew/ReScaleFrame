// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
namespace rsf::skyrim {
bool sampling_guard();
bool sampling_start(const rsf_game_host_services&);
void sampling_quiesce();
bool sampling_busy();
bool sampling_stop();
void finish_sampling();
}
