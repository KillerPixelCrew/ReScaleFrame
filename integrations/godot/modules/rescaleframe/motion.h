// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <stdint.h>

struct rsf_godot_motion_constants {
    float matrix[16], jitter[2];
    uint32_t size[2];
};
static_assert(sizeof(rsf_godot_motion_constants) == 80 &&
    offsetof(rsf_godot_motion_constants, jitter) == 64 && offsetof(rsf_godot_motion_constants, size) == 72);

struct rsf_godot_motion_calls {
    uint64_t (*shader)(void *context, uint32_t normalized);
    uint64_t (*pipeline)(void *context, uint64_t shader);
    uint64_t (*sampler)(void *context);
    void (*free_rid)(void *context, uint64_t resource);
    uint64_t (*texture)(void *context, uint32_t depth);
    void (*dispatch)(void *context, uint64_t shader, uint64_t pipeline, uint64_t sampler,
            uint64_t motion, uint64_t normalized, const rsf_godot_motion_constants *constants);
};
static_assert(sizeof(rsf_godot_motion_calls) == 6 * sizeof(void *));

extern "C" uint64_t rsf_godot_motion_resolve(const rsf_godot_motion_calls *calls, void *context,
        uint32_t normalized, const rsf_godot_motion_constants *facts, uint64_t *out_depth);
extern "C" void rsf_godot_motion_release(const rsf_godot_motion_calls *calls, void *context);
