// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <stdint.h>
#ifndef __cplusplus
#include <stdbool.h>
#endif

// The create_shader caller frame of one build, filled from motion_profile.rs.
typedef struct {
    uint32_t calls[2], call_length, source, length, entry, flags, defines, define_capacity;
} rsf_anomaly_shader_frame_layout;
typedef bool (*rsf_anomaly_shader_read)(const void*, uintptr_t address, uint8_t* out, size_t);
