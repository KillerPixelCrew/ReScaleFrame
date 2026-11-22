// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
#include <cstddef>

namespace rsf::skyrim {
struct NativeScreenFacts {
    bool readable, owned_main, primary_world, loading_menu, main_menu, frozen_background;
    uint32_t pause_count, requested_mode, running_mode;
    bool freeze_time, parent_cell;
};
static_assert(sizeof(NativeScreenFacts) == 24 && offsetof(NativeScreenFacts, pause_count) == 8);
}
