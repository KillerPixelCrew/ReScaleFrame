// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
namespace rsf::skyrim {
// Cold-only, exact-build forwarding hooks. Published code/cells remain process-lived.
bool steam_present_prepare(const rsf_game_host_services&) noexcept;
}
