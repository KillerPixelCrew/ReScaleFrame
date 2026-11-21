// SPDX-License-Identifier: MIT
#pragma once

#include <stddef.h>
#include <stdint.h>

// Private Rust/native bridge. Views are borrowed only during their callbacks.
struct rsf_ac7_velocity_instruction_view {
    const uint32_t* words;
    size_t count;
    uint32_t opcode;
    bool declaration;
};

struct rsf_ac7_velocity_operand_view {
    size_t token, end;
    uint32_t type, dimensions, immediate32;
    uint32_t registers[3];

    bool direct(uint32_t count) const { return dimensions == count && immediate32 == (1u << count) - 1; }
    uint32_t reg() const { return registers[0]; }
};

struct rsf_ac7_velocity_operand_edit {
    uint32_t old_count, type, count;
    uint32_t registers[2];

    void remap(const rsf_ac7_velocity_operand_view& operand, uint32_t reg)
    { *this = {1, operand.type, 1, {reg, 0}}; }
    void to_temp(uint32_t reg, uint32_t old_count = 1)
    { *this = {old_count, 0, 1, {reg, 0}}; }
};

static_assert(sizeof(rsf_ac7_velocity_instruction_view) == 24);
static_assert(sizeof(rsf_ac7_velocity_operand_view) == 40);
static_assert(sizeof(rsf_ac7_velocity_operand_edit) == 20);

extern "C" {
bool rsf_core_ac7_velocity_read_instructions(const uint8_t*, size_t, uint32_t profile,
    void* user, bool (*append)(void*, const rsf_ac7_velocity_instruction_view*));
bool rsf_core_ac7_velocity_write_instructions(const uint32_t*, size_t,
    void* user, uint8_t* (*resize)(void*, size_t));
bool rsf_core_ac7_velocity_read_operands(const uint32_t*, size_t,
    void* user, bool (*append)(void*, const rsf_ac7_velocity_operand_view*));
bool rsf_core_ac7_velocity_rewrite_operands(const uint32_t*, size_t, bool nested,
    void* user, bool (*select)(void*, const rsf_ac7_velocity_operand_view*, rsf_ac7_velocity_operand_edit*),
    bool (*store)(void*, const uint32_t*, size_t));
}
