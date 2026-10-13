// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <stdint.h>
#include <rescaleframe/game_renderer.h>
#ifdef __cplusplus
extern "C" {
#endif
struct rsf_frame_tap_geometry;
void rsf_ac7_velocity_set_log(void (*log)(void*,const char*),void* user);
void rsf_ac7_velocity_shader_seen(void* shader,uint32_t stage,const void* bytes,uint32_t size);
void rsf_ac7_velocity_scope(const rsf_game_render_pass* scope);
void rsf_ac7_velocity_geometry(const struct rsf_frame_tap_geometry* geometry);
int rsf_ac7_velocity_commit(void* context,const rsf_game_render_pass* pass);
void rsf_ac7_velocity_frame_end(void);
void rsf_ac7_velocity_release(void);
#ifdef __cplusplus
}
#endif
