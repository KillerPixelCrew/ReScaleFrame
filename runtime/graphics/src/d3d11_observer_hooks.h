/* SPDX-License-Identifier: MIT */
#pragma once
#include <stddef.h>
#include <stdint.h>

typedef struct rsf_observer11_probe_attempt {
    void* context;
    void* chain;
    void* device;
    int32_t result;
    uint32_t returned;
} rsf_observer11_probe_attempt;
_Static_assert(sizeof(rsf_observer11_probe_attempt) == 32, "probe attempt layout");
_Static_assert(offsetof(rsf_observer11_probe_attempt, result) == 24, "probe attempt result offset");
