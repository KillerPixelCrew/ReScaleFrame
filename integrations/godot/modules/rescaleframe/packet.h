// SPDX-License-Identifier: MIT
#pragma once

#include "api.h"

typedef struct rsf_godot_packet_facts {
    void *device, *queue, *instance, *physical_device;
    rsf_godot_vk_proc instance_proc, device_proc;
    void *color, *depth, *motion, *output;
    rsf_godot_vk_image vk_color, vk_depth, vk_motion, vk_output;
} rsf_godot_packet_facts;

typedef struct rsf_godot_driver_packet {
    uint32_t driver;
    void *queue;
    rsf_godot_vk_packet payload;
} rsf_godot_driver_packet;

typedef struct rsf_godot_packet_calls {
    void *(*begin)(void *context, uint32_t driver);
    void (*end_d3d12)(void *context);
    uint32_t (*handoff)(void *context, rsf_godot_vk_handoff *out);
    int (*render_d3d12)(void *device, void *queue, void *commands, const rsf_game_render_pass *pass);
    int (*render_vulkan)(void *commands, const rsf_godot_vk_packet *packet);
    int (*render_handoff)(void *commands, const rsf_godot_vk_packet *packet,
            const rsf_godot_vk_handoff *handoff, void **continuation);
} rsf_godot_packet_calls;

typedef struct rsf_godot_swapchain_calls {
    void (*capture)(const void *setup);
    uint32_t (*allow_proxy)(void *context);
    uint32_t (*create)(const void *setup, void **owner, uint64_t *chain);
} rsf_godot_swapchain_calls;

#define RSF_GODOT_CHAIN_SKIPPED 0u
#define RSF_GODOT_CHAIN_REFUSED 1u
#define RSF_GODOT_CHAIN_CREATED 2u

#ifdef __cplusplus
struct rsf_godot_pass_record;
extern "C" void rsf_godot_packet_prepare(const rsf_godot_pass_record *record,
        const rsf_godot_packet_facts *facts, rsf_godot_driver_packet *out);
extern "C" void rsf_godot_packet_execute(const rsf_godot_driver_packet *packet,
        const rsf_godot_packet_calls *calls, void *context);
extern "C" uint32_t rsf_godot_swapchain_setup(rsf_fgvk_setup *setup,
        const rsf_godot_swapchain_calls *calls, void *context, void **owner, uint64_t *chain);
extern "C" void rsf_godot_execution_pass(uint32_t role, uint64_t session, uint64_t source, uint64_t window,
        rsf_game_render_pass *out);
#endif
