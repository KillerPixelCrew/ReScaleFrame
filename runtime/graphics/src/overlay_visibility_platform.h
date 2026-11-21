/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>

enum {
    RSF_VISIBILITY_EXTERNAL = 0,
    RSF_VISIBILITY_WINDOW = 1,
    RSF_VISIBILITY_POLL = 2,
};

typedef void (*rsf_visibility_apply)(uint32_t visible, uint32_t source);

extern "C" {
uint32_t rsf_core_overlay_visible(void);
uint32_t rsf_core_overlay_toggle_key(void);
void rsf_core_overlay_visibility_set(uint32_t visible, uint64_t now, rsf_visibility_apply);
uint32_t rsf_core_overlay_toggle_press(uint32_t repeat, uint64_t now, rsf_visibility_apply);
void rsf_core_overlay_toggle_release(void);
void rsf_core_overlay_toggle_poll(uint32_t focused, uint32_t down, uint64_t now, rsf_visibility_apply);
}
