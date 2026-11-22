// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_renderer.h>
extern "C" bool rsf_unity_preview_region(int32_t left, int32_t top, int32_t width, int32_t height, bool visible);
extern "C" bool rsf_unity_preview_owns(void* window);
extern "C" void* rsf_unity_preview_parent();
extern "C" void rsf_unity_preview_hide();
bool rsf_unity_preview_prepare(void* device, void* queue, uint32_t width, uint32_t height);
bool rsf_unity_preview_present(void* resource, const rsf_game_render_pass& pass, bool common_state = false);
bool rsf_unity_preview_drained() noexcept;
void rsf_unity_preview_release() noexcept;
