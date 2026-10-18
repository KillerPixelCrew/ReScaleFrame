// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/unity_bridge.h>
#include <IUnityInterface.h>
void rsf_uvk_preload(IUnityInterfaces*);
void rsf_uvk_load(IUnityInterfaces*, int event_id);
bool rsf_uvk_render(int event_id, const rsf_unity_packet&, const rsf_game_host_services&, bool reconfigure);
bool rsf_uvk_drained();
void rsf_uvk_release();
bool rsf_uvk_create_window();
bool rsf_uvk_update_window(uint32_t width, uint32_t height);
bool rsf_uvk_owns_window(void*);
bool rsf_uvk_graphics(void** device, void** queue);
