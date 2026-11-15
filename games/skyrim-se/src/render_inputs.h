// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_api.h>
namespace rsf::skyrim {
// Synchronous native pre-HDR callback. Resources are retained only through this call.
bool reconstruct(void* manager, const rsf_game_host_services&);
}
