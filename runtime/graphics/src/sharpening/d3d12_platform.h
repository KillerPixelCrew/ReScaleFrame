// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>

// Private call from the D3D12 sharpening facade into Rust.
typedef struct rsf_sharpen12 rsf_sharpen12;

typedef struct rsf_sharpen12_call {
    void* commands;
    void* textures[4]; // Output, motion, depth, exposure.
    const void* options; // Sanitized native Options, borrowed for this call.
    const void* frame;   // Native Frame, borrowed for this call.
    uint32_t mode, slot;
} rsf_sharpen12_call;

#ifdef __cplusplus
extern "C" {
#endif
// The caller serializes access and retires a recording before reusing its slot.
uint32_t rsf_core_sharpen12_run(rsf_sharpen12**, const rsf_sharpen12_call*);
uint64_t rsf_core_sharpen12_dispatches(const rsf_sharpen12*);
void rsf_core_sharpen12_destroy(rsf_sharpen12*);
#ifdef __cplusplus
}
#endif
