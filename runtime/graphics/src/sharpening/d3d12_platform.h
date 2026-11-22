// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>

// Private contract between d3d12.cpp and d3d12_sharpening.rs.
typedef struct rsf_sharpen12 rsf_sharpen12;

typedef struct rsf_sharpen12_texture {
    uint64_t width;
    uint32_t height, format, dimension, samples, flags;
    uint16_t array_size, mip_levels;
} rsf_sharpen12_texture;

typedef struct rsf_sharpen12_call {
    void* commands;
    void* textures[4]; // Output, motion, depth, exposure.
    const void* options; // Sanitized native Options, borrowed for this call.
    const void* frame;   // Native Frame, borrowed for this call.
    uint32_t mode, active, slot;
} rsf_sharpen12_call;

typedef struct rsf_sharpen12_dispatch {
    void* root;
    void* pipeline;
    void* heap;
    void* scratch;
    rsf_sharpen12_texture descriptions[4];
    uint32_t mode, groups_x, groups_y;
} rsf_sharpen12_dispatch;

typedef struct rsf_sharpen12_platform {
    // Getters and creation leaves return one owned COM reference, or null.
    void* (*child_device)(void*);
    void (*describe)(void*, rsf_sharpen12_texture*);
    void* (*create_root)(void*);
    void* (*create_pipeline)(void*, void*, uint32_t);
    void* (*create_heap)(void*);
    void* (*create_scratch)(void*, uint32_t, uint32_t, uint32_t);
    void (*dispatch)(void*, const rsf_sharpen12_call*, const rsf_sharpen12_dispatch*);
    void (*copy_back)(void*, void*, void*);
} rsf_sharpen12_platform;

#ifdef __cplusplus
extern "C" {
#endif
// The caller serializes access and retires a recording before reusing its slot.
uint32_t rsf_core_sharpen12_run(rsf_sharpen12**, const rsf_sharpen12_platform*, const rsf_sharpen12_call*);
uint64_t rsf_core_sharpen12_dispatches(const rsf_sharpen12*);
void rsf_core_sharpen12_destroy(rsf_sharpen12*);
#ifdef __cplusplus
}
#endif
