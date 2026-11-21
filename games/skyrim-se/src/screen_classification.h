// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
#include <cstddef>

namespace rsf::skyrim {
struct NativeScreenFacts {
    bool readable{}, owned_main{}, primary_world{}, loading_menu{}, main_menu{}, frozen_background{};
    uint32_t pause_count{}, requested_mode{}, running_mode{};
    bool freeze_time{}, parent_cell{};
};
static_assert(sizeof(NativeScreenFacts) == 24 && offsetof(NativeScreenFacts, pause_count) == 8);
}

extern "C" rsf_screen_class rsf_skyrim_classify_screen(const rsf::skyrim::NativeScreenFacts*) noexcept;
extern "C" bool rsf_skyrim_rendered_source_eligible(bool, uint32_t, rsf_screen_class) noexcept;

namespace rsf::skyrim {
inline rsf_screen_class classify_screen(const NativeScreenFacts& facts) noexcept {
    return rsf_skyrim_classify_screen(&facts);
}
inline bool rendered_source_eligible(bool active_scope, uint32_t primary_world_entries, rsf_screen_class screen) noexcept {
    return rsf_skyrim_rendered_source_eligible(active_scope, primary_world_entries, screen);
}
}
