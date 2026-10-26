// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "servers/rendering/rendering_device_graph.h"
#include <array>

constexpr int VK_SUCCESS = 0;
constexpr int VK_ERROR_INITIALIZATION_FAILED = -3;
constexpr int VK_ERROR_OUT_OF_HOST_MEMORY = -1;
class RenderingDeviceDriverVulkan : public RenderingDeviceDriver {
public:
    struct BufferState { bool begun = false, ended = false; uint64_t timeline = 0, wait = 0, signal = 0; };
    std::array<BufferState, 16> state{};
    uint32_t family = 3, allocated = 1, semaphores = 0, begins = 0, splits = 0;
    bool timeline_enabled = true, fail_allocate = false, fail_semaphore = false, fail_begin = false;
    bool rescaleframe_graph_info(CommandBufferID id, uint32_t &out) {
        out = family;
        return timeline_enabled && id && id.id < state.size() && !state[id.id].timeline;
    }
    void *rescaleframe_begin(CommandBufferID id) { return reinterpret_cast<void *>(id.id * 16); }
    CommandBufferID command_buffer_create(CommandPoolID) { return fail_allocate ? CommandBufferID{} : CommandBufferID{++allocated}; }
    SemaphoreID semaphore_create() { return fail_semaphore ? SemaphoreID{} : SemaphoreID{++semaphores}; }
    bool command_buffer_begin(CommandBufferID id) {
        ++begins;
        if (fail_begin) return false;
        state[id.id] = {}; state[id.id].begun = true; return true;
    }
    void rescaleframe_graph_split(CommandBufferID before, CommandBufferID after, uint64_t timeline,
            uint64_t previous, uint64_t ready, uint64_t done) {
        ++splits;
        state[before.id].ended = true; state[before.id].timeline = timeline;
        state[before.id].wait = previous; state[before.id].signal = ready;
        state[after.id].timeline = timeline; state[after.id].wait = done;
    }
};
