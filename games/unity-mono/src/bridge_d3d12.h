/* SPDX-License-Identifier: MIT */
#pragma once
#include "bridge_state.h"

/* Unity SDK calls only. Returned COM pointers are borrowed for the callback. */
typedef struct rsf_unity_d3d12_engine {
    void* (*device)(void);
    void* (*queue)(void);
    void* (*swapchain)(void);
    void* (*frame_fence)(void);
    uint64_t (*next_frame_fence_value)(void);
    void* (*recording_list)(void);
    uint64_t (*execute)(void* list, uint32_t count, void* const* resources, const int32_t* states);
    void (*slot_reset)(uint32_t bridge, void* list, uint32_t slot);
} rsf_unity_d3d12_engine;

#ifdef __cplusplus
extern "C" {
#endif
void rsf_unity_d3d12_render_packet(const rsf_unity_d3d12_engine* engine,
    const rsf_game_host_services* host, uint32_t offset, const rsf_unity_packet* packet);
uint64_t rsf_unity_d3d12_completed(void);
void rsf_unity_d3d12_release_graphics(void);
#ifdef __cplusplus
}
#endif
