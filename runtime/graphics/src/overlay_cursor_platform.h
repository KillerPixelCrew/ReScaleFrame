/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>

enum {
    RSF_CURSOR_SET_POS, RSF_CURSOR_CLIP, RSF_CURSOR_GET_POS, RSF_CURSOR_SHOW,
    RSF_CURSOR_SET, RSF_CURSOR_RAW_DATA, RSF_CURSOR_RAW_BUFFER, RSF_CURSOR_HOOK_COUNT
};

struct rsf_overlay_cursor_ops {
    int32_t (*initialize)(void);
    int32_t (*create)(void* target, void* detour, void** original);
    int32_t (*enable)(void* target);
    int32_t (*disable)(void* target);
    int32_t (*remove)(void* target);
};

extern "C" {
void rsf_core_overlay_cursor_install(void* const* detours, const rsf_overlay_cursor_ops*);
int32_t rsf_core_overlay_cursor_retire(const rsf_overlay_cursor_ops*);
uint32_t rsf_core_overlay_cursor_ready(void);
void rsf_core_overlay_cursor_enter(void);
void rsf_core_overlay_cursor_leave(void);
void* rsf_core_overlay_cursor_forward(uint32_t hook);
}
