/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <rescaleframe/game_frame.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*rsf_ac7_graph_check_fn)(bool, const char*);
typedef uint32_t (*rsf_ac7_graph_screen_fn)(uint32_t*, uint32_t*, uint64_t*);
typedef struct rsf_ac7_graph_particle_replay {
    void* owner;
    rsf_ac7_graph_check_fn check;
    bool (*tag_beam)(const void*, const void*);
    bool (*tag_trail)(const void*, const void*, uint32_t);
    bool (*beam_originals)(void*, void*, void*);
    bool (*trail_originals)(void*, void*, void*);
} rsf_ac7_graph_particle_replay;

void rsf_ac7_graph_replay_camera(unsigned char* module, size_t size,
    rsf_ac7_graph_screen_fn screen, rsf_ac7_graph_check_fn check);
void rsf_ac7_graph_replay_particles(const rsf_ac7_graph_particle_replay* replay);

struct rsf_ac7_truesky_patches;
typedef void (*rsf_ac7_graph_sky_apply_fn)(void*);
typedef bool (*rsf_ac7_graph_sky_restore_fn)(void*);
void rsf_ac7_graph_replay_truesky(unsigned char* image, size_t size, void* owner,
    const struct rsf_ac7_truesky_patches* patches, void* handler,
    rsf_ac7_graph_sky_apply_fn apply, rsf_ac7_graph_sky_restore_fn restore,
    rsf_ac7_graph_check_fn check);

#ifdef __cplusplus
}
#endif
