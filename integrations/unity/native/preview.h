// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <rescaleframe/d3d12_window_presenter.h>

#ifdef __cplusplus
extern "C" {
#endif
bool rsf_unity_preview_region(int32_t left, int32_t top, int32_t width, int32_t height, bool visible);
bool rsf_unity_preview_owns(void* window);
void* rsf_unity_preview_parent(void);
void rsf_unity_preview_hide(void);
uint32_t rsf_unity_preview_prepare(void* device, void* queue, uint32_t width, uint32_t height,
    rsf_d3d12_window_observer observer);
uint32_t rsf_unity_preview_present(void* resource, const rsf_game_render_pass* pass, uint32_t common_state);
bool rsf_unity_preview_drained(void);
void rsf_unity_preview_release(void);
#ifdef __cplusplus
}
#endif
