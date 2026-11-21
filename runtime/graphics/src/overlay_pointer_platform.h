/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/overlay.h>

struct rsf_overlay_client_point { int32_t x, y, width, height; };
enum {
    RSF_TOUCH_DOWN = 1u,
    RSF_TOUCH_UP = 2u,
    RSF_TOUCH_CANCEL = 4u,
    RSF_TOUCH_POINTER = 8u
};

extern "C" {
void rsf_core_overlay_pointer_clear(void);
void rsf_core_overlay_pointer_raw(const rsf_overlay_client_point*);
void rsf_core_overlay_pointer_move(const rsf_overlay_client_point*, uint32_t cursor_freed);
void rsf_core_overlay_pointer_seed(const rsf_overlay_client_point*);
void rsf_core_overlay_pointer_button(uint32_t bit, uint32_t down);
void rsf_core_overlay_pointer_polled_buttons(uint32_t buttons);
void rsf_core_overlay_pointer_contact(const rsf_overlay_client_point*, uint32_t id, uint32_t flags);
int32_t rsf_core_overlay_input_collect(rsf_overlay_input*, uint32_t width, uint32_t height, float delta_seconds,
    uint32_t (*sample_cursor)(void* window, rsf_overlay_client_point*));
}
