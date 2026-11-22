// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <stdint.h>
#ifndef __cplusplus
#include <stdbool.h>
#endif

typedef struct {
    uint32_t calls[2], call_length, source, length, entry, flags, defines, define_capacity;
} rsf_anomaly_shader_frame_layout;
typedef bool (*rsf_anomaly_shader_read)(const void*, uintptr_t address, uint8_t* out, size_t);

#ifdef __cplusplus
#include "motion_profile.h"

namespace rsf::anomaly::motion {
static_assert(sizeof(rsf_anomaly_shader_frame_layout) == 36);
inline rsf_anomaly_shader_frame_layout shader_frame_layout(const MotionProfile& profile) noexcept
{
    const auto& frame = profile.frame;
    return {{profile.create_shader_calls[0].rva, profile.create_shader_calls[1].rva}, call_length,
        frame.source_offset, frame.length_offset, frame.entry_offset, frame.flags_offset, frame.defines_offset,
        frame_defines_capacity};
}
}
#endif
