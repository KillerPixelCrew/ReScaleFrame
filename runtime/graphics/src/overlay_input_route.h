/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/overlay_input.h>
#include "overlay_visibility_platform.h"

enum {
    RSF_OVERLAY_KEY_DOWN = 1u,
    RSF_OVERLAY_KEY_SYSTEM = 2u,
    RSF_OVERLAY_KEY_REPEAT = 4u
};

extern "C" {
uint32_t rsf_core_overlay_route_key(uint32_t key, uint32_t flags, uint32_t visible, rsf_visibility_apply);
uint32_t rsf_core_overlay_route_button(uint32_t key, uint32_t down, uint32_t visible);
void rsf_core_overlay_route_focus_lost(void);
void rsf_core_overlay_note_pointer(uint32_t received, uint32_t consumed);
void rsf_core_overlay_note_wheel(float notches);
void rsf_core_overlay_note_raw(void);
int32_t rsf_core_overlay_input_counters(rsf_overlay_input_counters*);
}
