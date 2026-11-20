// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/overlay_d3d12.h>

typedef void (*rsf_overlay12_log)(void*, const char*);
typedef struct rsf_overlay12_platform {
    void* (*initialize)(void* device, void* queue, void* chain, rsf_overlay12_log, void* user);
    uint32_t (*visible)(void);
    void (*set_visible)(uint32_t);
    uint64_t (*completed)(void* panel);
    uint32_t (*wait)(void* panel, uint64_t value);
    /* The returned backbuffer carries one reference through submission. */
    void* (*begin)(void* panel, uint32_t slot, void* chain, uint32_t* width, uint32_t* height);
    uint32_t (*layout)(void* panel, uint32_t slot, uint32_t width, uint32_t height,
        float elapsed, float hint_alpha, const rsf_overlay_stats*, rsf_overlay_intent*);
    uint32_t (*record)(void* panel, uint32_t slot, void* target, uint32_t width, uint32_t height);
    uint32_t (*submit)(void* panel, uint32_t slot, uint64_t value);
} rsf_overlay12_platform;

#ifdef __cplusplus
extern "C" {
#endif
uint32_t rsf_core_overlay12_visible(const rsf_overlay12_platform*);
void rsf_core_overlay12_toggle(const rsf_overlay12_platform*);
int rsf_core_overlay12_present(const rsf_overlay12_platform*, void*, void*, void*, const rsf_overlay_stats*,
    rsf_overlay_intent*, rsf_overlay12_log, void*);
#ifdef __cplusplus
}
#endif
