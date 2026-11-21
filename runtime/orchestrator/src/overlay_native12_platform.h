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

typedef struct rsf_overlay12_platform {
    /* On success, transfers the renderer and one fence reference to the Rust owner. */
    void* (*initialize)(void* device, void* queue, void* chain, rsf_overlay12_log, void* user, void** fence);
    uint32_t (*stop_input)(void);
    void (*destroy)(void* panel);
    uint32_t (*visible)(void);
    void (*set_visible)(uint32_t);
    uint64_t (*completed)(void* fence);
    uint32_t (*arm)(void* fence, void* event, uint64_t value);
    /* The returned backbuffer carries one reference through submission. */
    void* (*begin)(void* panel, uint32_t slot, void* chain, rsf_overlay12_target*);
    /* Return one owned reference. Upload creation also reports its actual capacity. */
    void* (*create_pipeline)(void* panel, uint32_t format);
    void* (*create_upload)(void* panel, uint64_t bytes, uint64_t* capacity);
    /* The panel lends its draw data until the next layout. */
    uint32_t (*layout)(void* panel, uint32_t width, uint32_t height,
        float elapsed, float hint_alpha, const rsf_overlay_stats*, rsf_overlay_intent*, rsf_overlay_draw_data*);
    uint32_t (*updates)(void* panel, rsf_overlay_texture_update*, uint32_t capacity);
    uint32_t (*frees)(void* panel, uint64_t*, uint32_t capacity);
    /* Both return one owned D3D12 resource reference. */
    void* (*create_texture)(void* panel, uint32_t width, uint32_t height);
    void* (*upload_texture)(void* panel, uint32_t slot, void* resource, const rsf_overlay_texture_update*);
    uint32_t (*record)(void* panel, uint32_t slot, const rsf_overlay12_draw*,
        void* const* textures, uint32_t texture_count, const rsf_overlay12_draw_call*, uint32_t call_count);
    void (*execute)(void* panel, uint32_t slot);
    uint32_t (*signal)(void* panel, void* fence, uint64_t value);
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
