// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
namespace rsf::skyrim {
bool resolution_owned();
void configure_resolution(void* state, const rsf_game_host_services&);
bool resolution_extent(uint32_t output_width, uint32_t output_height, uint32_t& width, uint32_t& height);
bool enter_output_resolution();
void leave_output_resolution();
void record_per_frame_flags(uint8_t flags);
bool output_resolution_ready();
}
