// SPDX-License-Identifier: MIT
#pragma once
#include "input_ownership.h"
#include "screen_classification.h"
#include <cstddef>

// Private records on the native detour's stack. Rust owns their transitions.
struct rsf_skyrim_cpu_render_state {
    uint64_t scope{};
    rsf_screen_class screen{RSF_SCREEN_UNKNOWN};
    uint32_t primary_world_entries{}, primary_count{};
    bool render_entered{}, simulation_ended{}, sampled{};
};

struct rsf_skyrim_cpu_frame_input {
    rsf::skyrim::InputStamp consumed{};
    uint32_t camera_updates{}, movement_calls{}, player_update_calls{}, player_camera_updates{};
    bool ambiguous{};
};

using rsf_skyrim_cpu_read = bool (*)(uintptr_t, void*, size_t);
using rsf_skyrim_cpu_menus = bool (*)(uintptr_t open, uintptr_t frozen, uintptr_t ui,
    uintptr_t loading_name, uintptr_t main_name, rsf::skyrim::NativeScreenFacts*);

extern "C" {
void rsf_skyrim_cpu_record_consumer(rsf_skyrim_cpu_frame_input*, const rsf::skyrim::InputStamp*,
    rsf::skyrim::CpuConsumer, bool mapped) noexcept;
bool rsf_skyrim_cpu_consumption_proven(const rsf_skyrim_cpu_frame_input*) noexcept;
uint64_t rsf_skyrim_cpu_next_source() noexcept;
rsf_screen_class rsf_skyrim_cpu_screen(uintptr_t main, bool primary, rsf_skyrim_cpu_menus) noexcept;
struct rsf_skyrim_cpu_render_receipt {
    uint64_t scope;
    rsf_screen_class screen;
};
bool rsf_skyrim_cpu_begin_render(rsf_skyrim_cpu_render_state*, rsf_screen_class,
    rsf_skyrim_cpu_render_receipt*) noexcept;
void rsf_skyrim_cpu_end_render(rsf_skyrim_cpu_render_state*, const rsf_skyrim_cpu_render_receipt*) noexcept;
bool rsf_skyrim_cpu_begin_world(rsf_skyrim_cpu_render_state*, const rsf_game_render_pass*,
    uintptr_t base, uintptr_t caller, uint32_t output) noexcept;
void rsf_skyrim_cpu_world_screen(rsf_skyrim_cpu_render_state*, rsf_screen_class) noexcept;
void rsf_skyrim_cpu_end_world(rsf_skyrim_cpu_render_state*) noexcept;
bool rsf_skyrim_cpu_render_identity(const rsf_skyrim_cpu_render_state*, rsf_game_render_pass*,
    uint64_t source, bool active) noexcept;
void rsf_skyrim_cpu_render_fact(rsf_game_render_pass*, uint64_t session) noexcept;
bool rsf_skyrim_cpu_sample_tick(rsf_skyrim_cpu_render_state*) noexcept;
void rsf_skyrim_cpu_frame_event(rsf_game_cpu_event*, const rsf_skyrim_cpu_render_state*,
    uint32_t stage, uint64_t session, uint64_t source, uint64_t qpc, uint64_t frequency) noexcept;
void rsf_skyrim_cpu_source_event(rsf_game_cpu_source_event*, uint32_t stage, uint64_t session,
    uint64_t source, uint64_t qpc, uint64_t frequency, uint64_t predecessor, uint64_t generation,
    uint32_t flags, uint32_t kinds, uint32_t reason, uint64_t poll_begin) noexcept;
void rsf_skyrim_cpu_input_event(rsf_game_cpu_event*, uint64_t session, uint32_t kinds,
    uint64_t qpc, uint64_t frequency) noexcept;
uint32_t rsf_skyrim_cpu_input_kinds(uintptr_t list, rsf_skyrim_cpu_read) noexcept;
}

static_assert(sizeof(rsf_skyrim_cpu_render_state) == 24);
static_assert(offsetof(rsf_skyrim_cpu_render_state, render_entered) == 20);
static_assert(sizeof(rsf_skyrim_cpu_frame_input) == 40 && offsetof(rsf_skyrim_cpu_frame_input, ambiguous) == 32);
static_assert(RSF_SCREEN_UNKNOWN == 0 && RSF_SCREEN_MENU == 1 && RSF_SCREEN_LOADING == 7 && RSF_SCREEN_GAMEPLAY == 8);
