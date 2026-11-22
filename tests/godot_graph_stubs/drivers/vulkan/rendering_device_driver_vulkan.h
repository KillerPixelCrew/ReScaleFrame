// SPDX-License-Identifier: MIT
#pragma once
#include "servers/rendering/rendering_device_graph.h"
constexpr int VK_ERROR_INITIALIZATION_FAILED = -3;
constexpr int VK_ERROR_OUT_OF_HOST_MEMORY = -1;

extern "C" uint32_t rsf_fixture_vk_graph_info(void *state, uint64_t id);
extern "C" void *rsf_fixture_vk_command(void *state, uint64_t id);
extern "C" uint64_t rsf_fixture_vk_allocate(void *state);
extern "C" uint64_t rsf_fixture_vk_semaphore(void *state);
extern "C" void rsf_fixture_vk_free_semaphore(void *state);
extern "C" uint32_t rsf_fixture_vk_begin(void *state, uint64_t id);
extern "C" void rsf_fixture_vk_split(void *state, uint64_t before, uint64_t after,
        uint64_t timeline, uint64_t previous, uint64_t ready, uint64_t done);

class RenderingDeviceDriverVulkan : public RenderingDeviceDriver {
    void *state;

public:
    explicit RenderingDeviceDriverVulkan(void *p_state) : state(p_state) {}
    bool rescaleframe_graph_info(CommandBufferID id, uint32_t &out) {
        out = rsf_fixture_vk_graph_info(state, id.id);
        return out != 0;
    }
    void *rescaleframe_begin(CommandBufferID id) { return rsf_fixture_vk_command(state, id.id); }
    CommandBufferID command_buffer_create(CommandPoolID) { return {rsf_fixture_vk_allocate(state)}; }
    SemaphoreID semaphore_create() { return {rsf_fixture_vk_semaphore(state)}; }
    void semaphore_free(SemaphoreID) { rsf_fixture_vk_free_semaphore(state); }
    bool command_buffer_begin(CommandBufferID id) { return rsf_fixture_vk_begin(state, id.id) != 0; }
    void rescaleframe_graph_split(CommandBufferID before, CommandBufferID after, uint64_t timeline,
            uint64_t previous, uint64_t ready, uint64_t done) {
        rsf_fixture_vk_split(state, before.id, after.id, timeline, previous, ready, done);
    }
};
