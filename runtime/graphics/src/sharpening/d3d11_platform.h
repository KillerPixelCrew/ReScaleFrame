// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/d3d11_state.h>

typedef struct rsf_sharpen11 rsf_sharpen11;

typedef struct rsf_sharpen11_draw {
    void* views[4];
    void* output;
    void* scratch;
    void* target;
    void* constants;
    void* shader;
    uint32_t extent[6];
    uint32_t exposure_texture;
    uint32_t mode;
} rsf_sharpen11_draw;

typedef struct rsf_sharpen11_platform {
    void* (*shader)(void*, uint32_t);
    void* (*constants)(void*);
    void (*scratch)(void*, uint32_t, uint32_t, uint32_t, uint32_t, void**);
    uint32_t (*save)(void*, rsf_d3d11_state*);
    void (*restore)(void*, rsf_d3d11_state*);
    void (*dispatch)(void*, const void*, const void*, const rsf_sharpen11_draw*);
} rsf_sharpen11_platform;

#ifdef __cplusplus
extern "C" {
#endif
rsf_sharpen11* rsf_core_sharpen11_create(void);
uint32_t rsf_core_sharpen11_run(rsf_sharpen11*, const rsf_sharpen11_platform*, void*, void* const*,
    uint32_t, const void*, const void*);
uint64_t rsf_core_sharpen11_count(const rsf_sharpen11*);
void rsf_core_sharpen11_destroy(rsf_sharpen11*);
#ifdef __cplusplus
}
#endif
