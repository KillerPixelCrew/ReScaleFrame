// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
#include <stdbool.h>
#include <stddef.h>

typedef struct rsf_skyrim_upload_observation {
    uint64_t session;
    uint32_t frame;
} rsf_skyrim_upload_observation;

_Static_assert(sizeof(rsf_skyrim_upload_observation) == 16 && offsetof(rsf_skyrim_upload_observation, frame) == 8,
    "Skyrim upload observation layout");
void rsf_skyrim_render_end_enter(rsf_game_host_services*);
void rsf_skyrim_render_end_leave(const rsf_game_host_services*);
bool rsf_skyrim_render_upload_enter(uintptr_t renderer, rsf_skyrim_upload_observation*);
void rsf_skyrim_render_upload_leave(const rsf_skyrim_upload_observation*, uint8_t flags);
void rsf_skyrim_render_jitter_observed(uintptr_t state);
