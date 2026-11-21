/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/overlay_input.h>
#include "overlay_visibility_platform.h"

enum {
    RSF_OVERLAY_MESSAGE_FORWARD = 0,
    RSF_OVERLAY_MESSAGE_CONSUME = 1,
    RSF_OVERLAY_MESSAGE_CONSUME_XBUTTON = 2,
    RSF_OVERLAY_MESSAGE_DEFAULT = 3
};

struct rsf_overlay_contact {
    int32_t x, y;
    uint32_t id, flags;
    int32_t kind;
};

struct rsf_overlay_message_ops {
    uint32_t (*pointer)(uint32_t id, rsf_overlay_contact*);
    uint32_t (*touch)(intptr_t handle, uint32_t count, rsf_overlay_contact*, uint64_t* now);
    void (*close_touch)(intptr_t handle);
    void (*contact)(void* window, const rsf_overlay_contact*, uint32_t screen_coordinates);
    void (*mouse_position)(void* window, intptr_t lparam);
    void (*mouse_button)(uint32_t key, uint32_t down);
    void (*raw_mouse)(void* window, intptr_t lparam);
    rsf_visibility_apply apply;
};

extern "C" {
uint32_t rsf_core_overlay_route_message(void* window, uint32_t message, uintptr_t wparam, intptr_t lparam,
    const rsf_overlay_message_ops*);
int32_t rsf_core_overlay_input_counters(rsf_overlay_input_counters*);
}
