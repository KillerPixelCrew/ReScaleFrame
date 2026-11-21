// SPDX-License-Identifier: MIT
#pragma once

#include <rescaleframe/d3d11_observer.h>
#include <cstddef>

struct MotionShaderFacts { uint32_t hash, constant_mask; };
struct MotionLayoutSnapshot {
    uint32_t count, copied;
    rsf_observer_layout_element elements[RSF_OBSERVER_MAX_LAYOUT_ELEMENTS];
};

using MotionDisassembleConstants = uint32_t(*)(const void* bytes, std::size_t size);

extern "C" {
// False means allocation failure. Invalid observations and budget refusals are handled.
bool rsf_ac7_capture_observe_shader(void* shader, uint32_t stage, const void* bytes, uint32_t size,
    MotionDisassembleConstants disassemble);
uint32_t rsf_ac7_capture_declared_constants(const unsigned char* text, std::size_t size);
MotionShaderFacts rsf_ac7_capture_shader_facts(void* shader, uint32_t stage);

bool rsf_ac7_capture_observe_layout(void* layout, const rsf_observer_layout_element* elements,
    uint32_t copied, uint32_t count);
bool rsf_ac7_capture_find_layout(void* layout, MotionLayoutSnapshot* out);
}

static_assert(sizeof(MotionShaderFacts) == 8 && offsetof(MotionShaderFacts, constant_mask) == 4);
static_assert(sizeof(rsf_observer_layout_element) == 20 && offsetof(rsf_observer_layout_element, per_instance) == 16);
static_assert(sizeof(MotionLayoutSnapshot) == 328 && offsetof(MotionLayoutSnapshot, elements) == 8);
