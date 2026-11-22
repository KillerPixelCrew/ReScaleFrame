// SPDX-License-Identifier: MIT
#pragma once
// Private, generated Rust/C++ render records. These are not DLL exports.
#include <stdint.h>

namespace rsf::fallout4 {
struct NativeScreen {
    uint32_t screen{}, facts{}, refusals{};
    uint32_t camera_index{UINT32_MAX};
    uintptr_t camera_state{};
};

struct NativeOutputOwner {
    uint64_t context{}, manager{}, triangles[3]{};
};

struct NativeResolutionReceipt {
    const char* stage;
    uintptr_t geometry[3];
    float input_ratio[2], ratio[2], raster[6];
    uint32_t frame, dirty;
    int32_t result;
    uint8_t viewport, active, partial;
    bool evaluation, published, input, output, changed, scalars, owner, shadow;
    uint32_t actual_count, actual_read;
    float actual_raster[6];
};

struct NativeDepthMathFailure {
    uint32_t index{};
    float actual{}, expected{}, relative_error{};
    double w[4]{}, minimum_w{}, range{};
};

// Texture slots of the reader lease. Slot 0 also holds the framebuffer texture.
enum NativeInputSlot : uint32_t {
    native_input_colour, native_input_motion, native_input_depth,
    native_input_refraction_normals, native_input_pipboy, native_input_vats,
    native_input_slot_count, native_input_no_slot = 0xffffffffu
};
}
