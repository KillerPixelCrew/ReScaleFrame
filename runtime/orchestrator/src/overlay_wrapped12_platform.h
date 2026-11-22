// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/overlay_d3d12.h>
#include "overlay_native12_platform.h"

typedef struct rsf_overlay11on12_device {
    void* device;
    void* context;
    void* interop;
} rsf_overlay11on12_device;

typedef struct rsf_overlay11on12_target {
    void* backbuffer;
    void* wrapped;
    void* view;
    uint32_t width, height;
} rsf_overlay11on12_target;

typedef struct rsf_overlay11on12_platform {
    /* Successful device and target calls transfer one reference per pointer. */
    int32_t (*create)(void* device, void* queue, rsf_overlay11on12_device*);
    uint32_t (*window)(void* chain, void** window);
    int (*start)(void* device, void* window, rsf_overlay12_log, void* user);
    int (*admit)(uint32_t show_hud, uint32_t* visible, float* hint);
    uint32_t (*target)(void* device, void* interop, void* chain, rsf_overlay11on12_target*);
    void (*acquire)(void* interop, void* wrapped);
    int (*draw)(void* context, void* view, uint32_t width, uint32_t height,
        const rsf_overlay_stats*, rsf_overlay_intent*, uint32_t visible, float hint);
    void (*release)(void* interop, void* wrapped);
    void (*flush)(void* context);
    void (*stop)(void* device);
} rsf_overlay11on12_platform;
