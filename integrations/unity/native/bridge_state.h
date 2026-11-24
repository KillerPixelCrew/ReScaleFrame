/* SPDX-License-Identifier: MIT */
#pragma once
/* The frame records of the shared Unity D3D12 render bridge, whose state is bridge_state.rs. The
   game plugin and the Unity package both link that archive. bridge_native.rs and
   bridge_d3d12.rs call it directly; only render_bridge.c, which keeps the Unity SDK calls,
   reaches back into it, to deactivate the bridge. */
#include <stdbool.h>
#include <rescaleframe/unity_bridge.h>

/* Packet storage, graphics command slots and source-submission ring sizes. */
#define RSF_UNITY_FRAMES_PACKETS 32u
#define RSF_UNITY_FRAMES_SLOTS 3u
#define RSF_UNITY_FRAMES_SOURCES 8u

#ifdef __cplusplus
extern "C" {
#endif
/* Producer state. Inactive bridges refuse packets, frames and CPU events. */
void rsf_unity_frames_activate(uint32_t enabled);
#ifdef __cplusplus
}
#endif