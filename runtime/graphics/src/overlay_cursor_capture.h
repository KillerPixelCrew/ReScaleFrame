/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>
#include <windows.h>

struct rsf_overlay_cursor_effects {
    int32_t (*get)(POINT*);
    void (*clip)(const RECT*);
    int32_t (*show)(int32_t);
    void (*arrow)(void);
};

extern "C" {
uint32_t rsf_core_overlay_cursor_captured(void);
uint32_t rsf_core_overlay_cursor_swallow_warp(void);
uint32_t rsf_core_overlay_cursor_clip_request(const RECT*);
uint32_t rsf_core_overlay_cursor_frozen(POINT*);
int32_t rsf_core_overlay_cursor_show(int32_t visible, int32_t (*original)(int32_t));
void rsf_core_overlay_cursor_capture(uint32_t visible, uint32_t restore_clip,
    const rsf_overlay_cursor_effects*);
void rsf_core_overlay_cursor_reset_capture(void);
}
