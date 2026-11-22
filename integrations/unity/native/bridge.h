/* SPDX-License-Identifier: MIT */
#pragma once
#include "bridge_state.h"

#define RSF_UNITY_BRIDGE_NONE 0u
#define RSF_UNITY_BRIDGE_D3D11 1u
#define RSF_UNITY_BRIDGE_D3D12 2u
#define RSF_UNITY_BRIDGE_VULKAN 3u

#ifdef __cplusplus
extern "C" {
#endif
uint32_t rsf_unity_package_bridge_load(void* interfaces);
void rsf_unity_package_bridge_unload(void);
uint32_t rsf_unity_package_bridge_open(uint32_t editor);
uint32_t rsf_unity_package_bridge_try_close(void);
void rsf_unity_package_bridge_finish_retirement(uint32_t commit);
void rsf_unity_bridge_activate(uint32_t enabled);
uint32_t rsf_unity_package_bridge_prepare(const rsf_game_host_services* host, rsf_unity_native_api* api);
int rsf_unity_set_engine_spatial(uint32_t spatial, uint32_t quality);
#ifdef __cplusplus
}
#endif
