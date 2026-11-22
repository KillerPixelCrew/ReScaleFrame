/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Factory2 for the D3D11 fixture and Factory4 for the shared FG fixture.
   A successful call transfers one reference to each output. */
int rsf_present_fixture_adapter(uint32_t vendor, uint32_t factory4, void** factory, void** adapter);
int rsf_present_fixture_device11(void* adapter, void** device, void** context);
int rsf_present_fixture_device12(void* adapter, void** device, void** queue);
int rsf_present_fixture_debug12(void);
int rsf_d3d11_fixture_proxy_smoke(const char* proxy_path);
int rsf_d3d11_fixture_borderless(void* chain, void* window);
int rsf_d3d11_fixture_clear_and_present(void* device, void* context, void* chain, uint32_t held_resize);
/* Transfers four SR textures followed by five hint textures. */
int rsf_d3d11_fixture_inputs(void* device, void* context, void** textures);
uint16_t rsf_d3d11_fixture_red_half(void* device, void* context, void* texture);
void rsf_d3d11_fixture_flip_source(void* device, void* context, void* target, uint64_t callback_frame);

typedef struct rsf_fg_fixture_inputs {
    void *cpu_heap, *gpu_heap, *color_heap, *depth, *motion;
    uint32_t stride;
} rsf_fg_fixture_inputs;
int rsf_fg_bridge_fixture_allocate(void* gpu, uint32_t mixed, uint32_t hot, rsf_fg_fixture_inputs*);
int rsf_fg_bridge_fixture_commands(void* gpu, void** allocator, void** list);
int rsf_fg_bridge_fixture_check_flip(void* readback, uint64_t frame);
uint32_t rsf_fg_bridge_fixture_query(void* chain, uint32_t* previous);
int rsf_fg_bridge_fixture_reset(void* allocator, void* list);
int rsf_fg_bridge_fixture_source_buffer(void* chain, void* allocator, void* list, void** texture);
int rsf_fg_bridge_fixture_execute(void* queue, void* list);
int rsf_fg_bridge_fixture_drain(void* queue, void* fence, void* event, uint64_t frame);

#ifdef __cplusplus
}
#endif
