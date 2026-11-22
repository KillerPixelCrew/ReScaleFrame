// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/overlay_d3d12.h>

typedef void (*rsf_overlay12_log)(void*, const char*);
enum { RSF_OVERLAY12_TEXTURE_LIMIT = 64 };
typedef struct rsf_overlay12_target {
    uint32_t width, height, format;
} rsf_overlay12_target;

typedef struct rsf_overlay12_draw_call {
    uint32_t descriptor, first_index, index_count;
    int32_t vertex_offset;
    int32_t clip[4];
} rsf_overlay12_draw_call;

typedef struct rsf_overlay12_draw {
    void* target;
    void* pipeline;
    void* vertices;
    void* indices;
    const void* vertex_data;
    const void* index_data;
    uint32_t width, height, vertex_bytes, index_bytes;
} rsf_overlay12_draw;

/* One Present slot: direct allocator, its closed graphics list, a shader-visible SRV heap of
   RSF_OVERLAY12_TEXTURE_LIMIT descriptors and a one-entry RTV heap. The Rust owner holds each
   reference; leaves borrow the table for one call. */
typedef struct rsf_overlay12_frame {
    void* allocator;
    void* commands;
    void* srv;
    void* rtv;
} rsf_overlay12_frame;

typedef struct rsf_overlay12_platform {
    void* (*window)(void* chain);
    /* On success, transfers the native renderer and one fence reference to the Rust owner. */
    void* (*initialize)(void* device, void* queue, void** fence);
    /* Creates one slot's allocator, closed list, SRV heap and RTV heap in that order, stopping at
       the first refusal. Every created object is one owned reference in the output, also when a
       later step fails; the rest are null. Returns nonzero only when all four exist. */
    uint32_t (*create_frame)(void* panel, rsf_overlay12_frame*);
    uint32_t (*install_input)(void* window, rsf_overlay12_log, void* user);
    uint32_t (*stop_input)(void);
    void (*destroy)(void* panel);
    uint32_t (*visible)(void);
    void (*set_visible)(uint32_t);
    uint64_t (*completed)(void* fence);
    uint32_t (*arm)(void* fence, void* event, uint64_t value);
    /* Resets the slot's allocator and list. The returned backbuffer carries one reference through
       submission. */
    void* (*begin)(const rsf_overlay12_frame*, void* chain, rsf_overlay12_target*);
    /* Return one owned reference. Upload creation also reports its actual capacity. */
    void* (*create_pipeline)(void* panel, uint32_t format);
    void* (*create_upload)(void* panel, uint64_t bytes, uint64_t* capacity);
    uint32_t (*collect_input)(rsf_overlay_input*, uint32_t width, uint32_t height, float elapsed);
    /* Both return one owned D3D12 resource reference. */
    void* (*create_texture)(void* panel, uint32_t width, uint32_t height);
    void* (*upload_texture)(void* panel, const rsf_overlay12_frame*, void* resource,
        const rsf_overlay_texture_update*);
    uint32_t (*record)(void* panel, const rsf_overlay12_frame*, const rsf_overlay12_draw*,
        void* const* textures, uint32_t texture_count, const rsf_overlay12_draw_call*, uint32_t call_count);
    void (*execute)(void* panel, const rsf_overlay12_frame*);
    uint32_t (*signal)(void* panel, void* fence, uint64_t value);
} rsf_overlay12_platform;
