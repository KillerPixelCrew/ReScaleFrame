/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/overlay_input.h>

struct rsf_overlay_window_ops {
    void (*install_cursor)(void);
    int32_t (*retire_cursor)(uint32_t visible, uint32_t window_alive);
    void (*log)(rsf_overlay_input_log_fn, void* user, const char* message);
};

extern "C" {
int32_t rsf_core_overlay_input_install(void* window, const rsf_overlay_input_options*,
    intptr_t procedure, const rsf_overlay_window_ops*);
int32_t rsf_core_overlay_input_uninstall(intptr_t procedure, const rsf_overlay_window_ops*);
void* rsf_core_overlay_input_window(void);
intptr_t rsf_core_overlay_input_original(void);
void rsf_core_overlay_input_log(const char* message);
void rsf_core_overlay_input_hold(uint32_t key);
uint32_t rsf_core_overlay_input_release(uint32_t key);
void rsf_core_overlay_input_clear_holds(void);
}
