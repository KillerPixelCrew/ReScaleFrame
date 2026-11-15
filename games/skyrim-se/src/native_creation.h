// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_api.h>
namespace rsf::skyrim {
bool native_creation_prepare(const rsf_game_host_services&);
bool native_creation_start();
void native_creation_quiesce();
bool native_creation_stop();
bool native_creation_busy();
}
