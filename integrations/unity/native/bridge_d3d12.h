/* SPDX-License-Identifier: MIT */
#pragma once
#include "bridge_state.h"

#define RSF_UNITY_D3D12_EVENT_FLUSH 2u
#define RSF_UNITY_D3D12_EVENT_SYNC 4u
#define RSF_UNITY_D3D12_EVENT_MODIFIES 8u

/* Unity SDK calls only. Returned COM pointers are borrowed for the callback. */
typedef struct rsf_unity_d3d12_engine {
    void* (*device)(void);
    void* (*queue)(void);
    void* (*swapchain)(void);
    void* (*frame_fence)(void);
    uint64_t (*next_frame_fence_value)(void);
    void* (*recording_list)(void);
    uint64_t (*execute)(void* list, uint32_t count, void* const* resources, const int32_t* states);
} rsf_unity_d3d12_engine;

#ifdef __cplusplus
extern "C" {
#endif
const rsf_unity_d3d12_engine* rsf_unity_d3d12_sdk_engine(void);
bool rsf_unity_d3d12_sdk_available(void);
bool rsf_unity_d3d12_sdk_lookup(void* address);
void rsf_unity_d3d12_sdk_load(void* interfaces, bool package);
void rsf_unity_d3d12_sdk_unload(void);
int rsf_unity_d3d12_sdk_reserve_events(void);
void rsf_unity_d3d12_sdk_configure_event(int id, bool allow_queue, uint32_t flags, bool bind_texture);
void rsf_unity_d3d12_configure_events(bool package);
#ifdef __cplusplus
}
#endif
