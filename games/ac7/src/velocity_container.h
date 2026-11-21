// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

struct rsf_ac7_velocity_semantic_view {
    const char* name;
    size_t name_size;
    uint32_t index, system, type, reg;
    uint8_t mask, used;
};
static_assert(sizeof(rsf_ac7_velocity_semantic_view) == 40);

extern "C" bool rsf_core_ac7_velocity_signature(const uint8_t*, size_t, bool output,
    void* user, bool (*append)(void*, const rsf_ac7_velocity_semantic_view*)) noexcept;
