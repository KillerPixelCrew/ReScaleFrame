// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>

namespace rsf::skyrim {
struct NativeScreenFacts {
    bool readable{}, owned_main{}, primary_world{}, loading_menu{}, main_menu{}, frozen_background{};
    uint32_t pause_count{}, requested_mode{}, running_mode{};
    bool freeze_time{}, parent_cell{};
};
inline rsf_screen_class classify_screen(const NativeScreenFacts& facts) noexcept {
    if (!facts.readable || !facts.owned_main) return RSF_SCREEN_UNKNOWN;
    if (facts.loading_menu) return RSF_SCREEN_LOADING;
    if (facts.main_menu || facts.pause_count || facts.frozen_background) return RSF_SCREEN_MENU;
    if (!facts.primary_world || facts.freeze_time || !facts.parent_cell ||
        facts.requested_mode > 1 || facts.running_mode > 1) return RSF_SCREEN_UNKNOWN;
    return RSF_SCREEN_GAMEPLAY;
}
inline bool rendered_source_eligible(bool active_scope, uint32_t primary_world_entries, rsf_screen_class screen) noexcept {
    return active_scope && primary_world_entries == 1 && screen == RSF_SCREEN_GAMEPLAY;
}
}
