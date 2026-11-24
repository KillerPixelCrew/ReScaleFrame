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

/* A reconstruction frame admitted into command ring slot `slot`. A nonzero generation
   identifies a pending change to acknowledge after delivery. */
typedef struct rsf_unity_frame_admission {
    uint32_t slot, pending_generation;
} rsf_unity_frame_admission;

/* A recorded source frame whose engine submission is not yet confirmed to the host. */
typedef struct rsf_unity_source_submission {
    uint64_t session_id, source_frame_id, view_key;
} rsf_unity_source_submission;

#ifdef __cplusplus
extern "C" {
#endif
/* Producer state. Inactive bridges refuse packets, frames and CPU events. */
void rsf_unity_frames_activate(uint32_t enabled);
#ifdef __cplusplus
}
#endif