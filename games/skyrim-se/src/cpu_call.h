// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

#define RSF_SKYRIM_CPU_STORAGE_WORDS 40

enum rsf_skyrim_cpu_hook {
    RSF_SKYRIM_CPU_TICK,
    RSF_SKYRIM_CPU_POLL,
    RSF_SKYRIM_CPU_RENDER,
    RSF_SKYRIM_CPU_DISPATCH,
    RSF_SKYRIM_CPU_WORLD,
    RSF_SKYRIM_CPU_CONTROLS,
    RSF_SKYRIM_CPU_CAMERA,
    RSF_SKYRIM_CPU_SUBMIT,
    RSF_SKYRIM_CPU_JOIN,
    RSF_SKYRIM_CPU_NAMED_POLL,
    RSF_SKYRIM_CPU_MOVEMENT,
    RSF_SKYRIM_CPU_PLAYER_UPDATE,
    RSF_SKYRIM_CPU_PLAYER_CAMERA,
    RSF_SKYRIM_CPU_HOOK_COUNT
};
typedef enum rsf_skyrim_cpu_hook rsf_skyrim_cpu_hook;

enum rsf_skyrim_observer_hook {
    RSF_SKYRIM_OBSERVER_JITTER,
    RSF_SKYRIM_OBSERVER_PROCESS,
    RSF_SKYRIM_OBSERVER_RESOLUTION,
    RSF_SKYRIM_OBSERVER_END,
    RSF_SKYRIM_OBSERVER_UPLOAD,
    RSF_SKYRIM_OBSERVER_HOOK_COUNT
};
typedef enum rsf_skyrim_observer_hook rsf_skyrim_observer_hook;

struct rsf_skyrim_cpu_call {
    uintptr_t owner;
    uintptr_t argument;
    uintptr_t caller;
    uint64_t storage[RSF_SKYRIM_CPU_STORAGE_WORDS];
};
typedef struct rsf_skyrim_cpu_call rsf_skyrim_cpu_call;

// Only the three facts produced by the guarded native calls cross this boundary.
struct rsf_skyrim_menu_observation {
    bool loading_menu;
    bool main_menu;
    bool frozen_background;
};
typedef struct rsf_skyrim_menu_observation rsf_skyrim_menu_observation;
