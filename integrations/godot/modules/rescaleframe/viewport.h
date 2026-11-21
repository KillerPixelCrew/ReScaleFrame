// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>

enum : uint32_t {
    RSF_GODOT_VIEWPORT_NATIVE = 0,
    RSF_GODOT_VIEWPORT_SPATIAL = 1,
    RSF_GODOT_VIEWPORT_TEMPORAL = 2,
};

// Private object linkage inside the SCons module. Matches viewport.rs.
extern "C" uint32_t rsf_godot_viewport_mode(uint32_t requested_sr);
extern "C" float rsf_godot_viewport_initial_scale(uint32_t mode, uint32_t quality);
