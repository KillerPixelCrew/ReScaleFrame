// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_renderer.h>
bool rsf_unity_preview_region(int left, int top, int width, int height, bool visible);
bool rsf_unity_preview_enabled();
bool rsf_unity_preview_owns(void* window);
void* rsf_unity_preview_parent();
bool rsf_unity_preview_prepare(void* device, void* queue, uint32_t width, uint32_t height);
bool rsf_unity_preview_present(void* resource, const rsf_game_render_pass& pass, bool common_state = false);
void rsf_unity_preview_hide();
uint64_t rsf_unity_preview_base_frames();
