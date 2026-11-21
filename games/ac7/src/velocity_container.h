// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

struct rsf_ac7_velocity_chunk_view {
    uint8_t tag[4];
    const uint8_t* bytes;
    size_t size;
};
struct rsf_ac7_velocity_semantic_view {
    const char* name;
    size_t name_size;
    uint32_t index, system, type, reg;
    uint8_t mask, used;
};
static_assert(sizeof(rsf_ac7_velocity_chunk_view) == 24);
static_assert(sizeof(rsf_ac7_velocity_semantic_view) == 40);

extern "C" {
bool rsf_core_ac7_velocity_read_container(const void* bytes, size_t size, void* user,
    bool (*append)(void*, const rsf_ac7_velocity_chunk_view*));
bool rsf_core_ac7_velocity_write_container(const rsf_ac7_velocity_chunk_view*, size_t count,
    void* user, uint8_t* (*resize)(void*, size_t));
bool rsf_core_ac7_velocity_read_signature(const uint8_t* bytes, size_t size, bool streams,
    void* user, bool (*append)(void*, const rsf_ac7_velocity_semantic_view*));
bool rsf_core_ac7_velocity_write_signature(const rsf_ac7_velocity_semantic_view*, size_t count,
    bool streams, void* user, uint8_t* (*resize)(void*, size_t));
}
