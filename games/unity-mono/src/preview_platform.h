// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stdint.h>
#include <rescaleframe/game_renderer.h>

struct rsf_unity_preview_rect {
    int32_t left, top, right, bottom;
};
struct rsf_unity_preview_texture {
    uint64_t width;
    uint32_t height, format;
};
typedef void (*rsf_unity_preview_candidate)(void* user, void* window, const rsf_unity_preview_rect*);
struct rsf_unity_preview_leaves {
    void (*enumerate)(void* user, rsf_unity_preview_candidate);
    void* (*create_window)(void* parent, int32_t width, int32_t height);
    bool (*position)(void* window, void* parent, int32_t left, int32_t top, int32_t width, int32_t height);
    void (*show)(void* window, bool visible, bool asynchronous);
    bool (*bind_gpu)(void* device, void* queue);
    bool (*create_chain)(void* window, uint32_t width, uint32_t height);
    bool (*chain_extent)(uint32_t* width, uint32_t* height);
    bool (*resize_chain)(uint32_t width, uint32_t height);
    void (*describe)(void* texture, rsf_unity_preview_texture*);
    bool (*wait)(uint64_t value);
    bool (*record)(uint32_t slot, void* texture, bool common_state);
    bool (*signal)(uint64_t value);
    bool (*present)(const rsf_game_render_pass*);
};
static_assert(sizeof(rsf_unity_preview_rect) == 16 && sizeof(rsf_unity_preview_texture) == 16);
static_assert(sizeof(rsf_unity_preview_leaves) == 104);

extern "C" {
bool rsf_core_unity_preview_region(int32_t left, int32_t top, int32_t width, int32_t height, bool show,
    const rsf_unity_preview_leaves*);
bool rsf_core_unity_preview_enabled();
bool rsf_core_unity_preview_owns(void* window);
void* rsf_core_unity_preview_parent();
void rsf_core_unity_preview_hide(const rsf_unity_preview_leaves*);
uint64_t rsf_core_unity_preview_base_frames();
bool rsf_core_unity_preview_prepare(void* device, void* queue, uint32_t width, uint32_t height,
    const rsf_unity_preview_leaves*);
bool rsf_core_unity_preview_present(void* texture, const rsf_game_render_pass*, bool common_state,
    const rsf_unity_preview_leaves*);
}
