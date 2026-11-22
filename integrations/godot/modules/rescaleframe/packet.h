// SPDX-License-Identifier: MIT
#pragma once

#include "api.h"

typedef struct rsf_godot_packet_sources {
    void *(*device)(void *context);
    void *(*queue)(void *context);
    void (*vk_context)(void *context, void **instance, void **physical_device);
    void (*vk_procs)(rsf_godot_vk_proc *instance, rsf_godot_vk_proc *device);
    uint64_t (*texture)(void *context, uint64_t rid);
    void (*vk_image)(void *context, uint64_t rid, rsf_godot_vk_image *out);
} rsf_godot_packet_sources;

typedef struct rsf_godot_driver_packet {
    uint32_t driver;
    void *queue;
    rsf_godot_vk_packet payload;
} rsf_godot_driver_packet;

typedef struct rsf_godot_packet_calls {
    void *(*begin_d3d12)(void *context);
    void *(*begin_vulkan)(void *context);
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

typedef struct rsf_godot_present_calls {
    uint32_t (*proxy_at)(void *context, uint32_t index);
    uint32_t (*window_at)(void *context, uint32_t index, uint64_t *window);
    uint32_t (*marker)(uint64_t window, uint32_t begin);
    int32_t (*native_batch)(void *context);
    int32_t (*proxy_batch)(void *context);
    void (*trace_mixed)(void *context);
    int32_t (*fan_out)(void *context);
    int32_t (*present_item)(void *context, uint32_t index);
} rsf_godot_present_calls;

typedef int32_t (*rsf_godot_graph_split_fn)(void *, void *, uint64_t, uint64_t, uint64_t, uint64_t, void **);
typedef struct rsf_godot_graph_calls {
    void *(*current)(void *scope);
    uint64_t (*pooled)(void *scope);
    uint64_t (*create_semaphore)(void *scope);
    uint64_t (*create_command)(void *scope);
    void (*free_semaphore)(void *scope, uint64_t semaphore);
    void (*append_pair)(void *scope, uint64_t command, uint64_t semaphore);
    uint32_t (*begin)(void *scope, uint64_t next);
    void (*labels)(void *scope, uint32_t reopen);
    void (*consume)(void *scope, uint64_t next, uint64_t timeline, uint64_t previous_done, uint64_t ready, uint64_t done);
    void *(*continuation)(void *scope);
} rsf_godot_graph_calls;

#define RSF_GODOT_CHAIN_SKIPPED 0u
#define RSF_GODOT_CHAIN_REFUSED 1u
#define RSF_GODOT_CHAIN_CREATED 2u

#ifdef __cplusplus
struct rsf_godot_pass_record;
extern "C" void rsf_godot_packet_prepare(const rsf_godot_pass_record *record,
        const rsf_godot_packet_sources *sources, void *context, const uint64_t (*textures)[4],
        rsf_godot_driver_packet *out);
extern "C" void rsf_godot_packet_execute(const rsf_godot_driver_packet *packet,
        const rsf_godot_packet_calls *calls, void *context);
extern "C" uint32_t rsf_godot_swapchain_setup(rsf_fgvk_setup *setup,
        const rsf_godot_swapchain_calls *calls, void *context, void **owner, uint64_t *chain);
extern "C" void rsf_godot_graph_pack(rsf_godot_vk_handoff *out, uint32_t family, void *driver,
        rsf_godot_graph_split_fn split);
extern "C" int32_t rsf_godot_graph_split(const rsf_godot_graph_calls *calls, void *scope, void *current,
        uint64_t timeline, uint64_t previous_done, uint64_t ready, uint64_t done, void **out);
extern "C" int32_t rsf_godot_present_batch(const rsf_godot_present_calls *calls, void *context,
        uint32_t count, uint32_t has_extensions, uint32_t submitted);
extern "C" void rsf_godot_execution_pass(uint32_t role, uint64_t session, uint64_t source, uint64_t window,
        rsf_game_render_pass *out);
#endif
