/* SPDX-License-Identifier: MIT */
#pragma once
#include <stddef.h>
#include <stdint.h>

struct rsf_observer11_probe_attempt {
    void* context;
    void* chain;
    void* device;
    int32_t result;
    uint32_t returned;
};
static_assert(sizeof(rsf_observer11_probe_attempt) == 32);
static_assert(offsetof(rsf_observer11_probe_attempt, result) == 24);
