// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
namespace rsf::skyrim {
// Synchronous native pre-HDR callback. Resources are retained only through this call.
bool reconstruct(void* manager, const rsf_game_host_services&);
uint32_t renderer_thread();
}
