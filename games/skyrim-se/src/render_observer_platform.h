// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
#include <stdbool.h>
#include <stddef.h>

typedef struct rsf_skyrim_upload_observation {
    uint64_t session;
    uint32_t frame;
} rsf_skyrim_upload_observation;

#ifdef __cplusplus
#define RSF_SKYRIM_NOEXCEPT noexcept
#define RSF_SKYRIM_ASSERT static_assert
extern "C" {
#else
#define RSF_SKYRIM_NOEXCEPT
#define RSF_SKYRIM_ASSERT _Static_assert
#endif
RSF_SKYRIM_ASSERT(sizeof(rsf_skyrim_upload_observation) == 16 && offsetof(rsf_skyrim_upload_observation, frame) == 8,
    "Skyrim upload observation layout");
void rsf_skyrim_render_end_enter(rsf_game_host_services*) RSF_SKYRIM_NOEXCEPT;
void rsf_skyrim_render_end_leave(const rsf_game_host_services*) RSF_SKYRIM_NOEXCEPT;
bool rsf_skyrim_render_upload_enter(uintptr_t renderer, rsf_skyrim_upload_observation*) RSF_SKYRIM_NOEXCEPT;
void rsf_skyrim_render_upload_leave(const rsf_skyrim_upload_observation*, uint8_t flags) RSF_SKYRIM_NOEXCEPT;
void rsf_skyrim_render_jitter_observed(uintptr_t state) RSF_SKYRIM_NOEXCEPT;
void rsf_skyrim_reset_render_observations(void) RSF_SKYRIM_NOEXCEPT;
bool rsf_skyrim_render_accept_window(void* window) RSF_SKYRIM_NOEXCEPT;
#ifdef __cplusplus
}
#endif
#undef RSF_SKYRIM_ASSERT
#undef RSF_SKYRIM_NOEXCEPT
