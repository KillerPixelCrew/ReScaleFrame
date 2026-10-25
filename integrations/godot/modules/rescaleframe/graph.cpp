// SPDX-License-Identifier: GPL-3.0-only
#include "graph.h"
#include "drivers/vulkan/rendering_device_driver_vulkan.h"

namespace RSFGodot {
thread_local GraphScope *GraphScope::active = nullptr;

bool GraphScope::handoff(RenderingDeviceDriver *driver, RenderingDeviceDriver::CommandBufferID commands,
        rsf_godot_vk_handoff &out) {
    if (!active || active->driver != driver || active->commands != commands) return false;
    auto *vk = static_cast<RenderingDeviceDriverVulkan *>(driver);
    uint32_t family = 0;
    if (!vk->rescaleframe_graph_info(commands, family)) return false;
    out = {sizeof(out), RSF_GODOT_VK_HANDOFF_ABI_VERSION, 1, family, driver, split_record};
    return true;
}

int32_t GraphScope::split_record(void *user, void *current_commands, uint64_t timeline,
        uint64_t previous_done, uint64_t ready, uint64_t done, void **continuation_commands) {
    if (continuation_commands) *continuation_commands = nullptr;
    auto *scope = active;
    if (!scope || scope->driver != user || !continuation_commands || !timeline ||
            ready <= previous_done || done <= ready) return VK_ERROR_INITIALIZATION_FAILED;
    auto *vk = static_cast<RenderingDeviceDriverVulkan *>(scope->driver);
    uint32_t family = 0;
    if (!vk->rescaleframe_graph_info(scope->commands, family) ||
            vk->rescaleframe_begin(scope->commands) != current_commands) return VK_ERROR_INITIALIZATION_FAILED;

    // Use the graph's normal chained-buffer storage. Its final engine fence owns
    // every section and its binary links retain transfer-worker/acquire ordering.
    auto &pool = scope->pool;
    if (pool.buffers_used >= pool.buffers.size()) {
        const auto next = vk->command_buffer_create(pool.pool);
        if (!next) return VK_ERROR_OUT_OF_HOST_MEMORY;
        const auto semaphore = vk->semaphore_create();
        if (!semaphore) return VK_ERROR_OUT_OF_HOST_MEMORY;
        pool.buffers.push_back(next);
        pool.semaphores.push_back(semaphore);
    }
    const auto next = pool.buffers[pool.buffers_used];
    // All fallible allocation/begin work precedes consumption of the old buffer.
    if (!vk->command_buffer_begin(next)) return VK_ERROR_INITIALIZATION_FAILED;
    scope->change_labels(scope->labels, false);
    vk->rescaleframe_graph_split(scope->commands, next, timeline, previous_done, ready, done);
    ++pool.buffers_used;
    scope->commands = next;
    scope->change_labels(scope->labels, true);
    *continuation_commands = vk->rescaleframe_begin(next);
    CRASH_COND_MSG(!*continuation_commands, "ReScaleFrame lost its graph continuation after consuming the source buffer.");
    return VK_SUCCESS;
}
}
