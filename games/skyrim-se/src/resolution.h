// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_api.h>
namespace rsf::skyrim {
bool resolution_owned();
void configure_resolution(void* state, const rsf_game_host_services&);
bool resolution_extent(uint32_t output_width, uint32_t output_height, uint32_t& width, uint32_t& height);
}
