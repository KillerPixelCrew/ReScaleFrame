/* SPDX-License-Identifier: MIT */
#pragma once
#include "bridge_state.h"

#ifdef __cplusplus
extern "C" {
#endif
/* The caller holds a frame callback and the queued packet lease. */
void rsf_unity_d3d11_render_packet(const rsf_game_host_services* host, uint32_t offset,
    bool editor, const rsf_unity_packet* packet);
/* Transfers device and queue references while the caller holds a frame callback. */
bool rsf_unity_d3d11_graphics(void** device, void** queue);
void rsf_unity_d3d11_open(void);
bool rsf_unity_d3d11_try_close(void);
bool rsf_unity_d3d11_release(void);
#ifdef __cplusplus
}
#endif
