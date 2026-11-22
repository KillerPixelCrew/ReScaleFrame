// SPDX-License-Identifier: MIT
#include "graph.h"
#include "packet.h"
#include "drivers/vulkan/rendering_device_driver_vulkan.h"

namespace RSFGodot {
static_assert(VK_ERROR_INITIALIZATION_FAILED == -3 && VK_ERROR_OUT_OF_HOST_MEMORY == -1);

thread_local GraphScope *GraphScope::active = nullptr;

bool GraphScope::handoff(RenderingDeviceDriver *driver, RenderingDeviceDriver::CommandBufferID commands,
        rsf_godot_vk_handoff &out) {
    if (!active || active->driver != driver || active->commands != commands) return false;
    auto *vk = static_cast<RenderingDeviceDriverVulkan *>(driver);
    uint32_t family = 0;
    if (!vk->rescaleframe_graph_info(commands, family)) return false;
    rsf_godot_graph_pack(&out, family, driver, split_record);
    return true;
}

int32_t GraphScope::split_record(void *user, void *current_commands, uint64_t timeline,
        uint64_t previous_done, uint64_t ready, uint64_t done, void **continuation_commands) {
    static constexpr rsf_godot_graph_calls calls{
        [](void *context) -> void * {
            auto &scope = *static_cast<GraphScope *>(context);
            auto *vk = static_cast<RenderingDeviceDriverVulkan *>(scope.driver);
            uint32_t family;
            return vk->rescaleframe_graph_info(scope.commands, family) ? vk->rescaleframe_begin(scope.commands) : nullptr;
        },
        [](void *context) -> uint64_t {
            const auto &pool = static_cast<GraphScope *>(context)->pool;
            return pool.buffers_used < pool.buffers.size() ? pool.buffers[pool.buffers_used].id : 0;
        },
        [](void *context) -> uint64_t {
            auto &scope = *static_cast<GraphScope *>(context);
            auto *vk = static_cast<RenderingDeviceDriverVulkan *>(scope.driver);
            return vk->semaphore_create().id;
        },
        [](void *context) -> uint64_t {
            auto &scope = *static_cast<GraphScope *>(context);
            auto *vk = static_cast<RenderingDeviceDriverVulkan *>(scope.driver);
            return vk->command_buffer_create(scope.pool.pool).id;
        },
        [](void *context, uint64_t id) {
            auto &scope = *static_cast<GraphScope *>(context);
            auto *vk = static_cast<RenderingDeviceDriverVulkan *>(scope.driver);
            RenderingDeviceDriver::SemaphoreID semaphore;
            semaphore.id = id;
            vk->semaphore_free(semaphore);
        },
        [](void *context, uint64_t command_id, uint64_t semaphore_id) {
            auto &pool = static_cast<GraphScope *>(context)->pool;
            RenderingDeviceDriver::CommandBufferID command;
            RenderingDeviceDriver::SemaphoreID semaphore;
            command.id = command_id;
            semaphore.id = semaphore_id;
            pool.buffers.push_back(command);
            pool.semaphores.push_back(semaphore);
        },
        [](void *context, uint64_t id) -> uint32_t {
            const auto &scope = *static_cast<GraphScope *>(context);
            RenderingDeviceDriver::CommandBufferID next;
            next.id = id;
            return static_cast<RenderingDeviceDriverVulkan *>(scope.driver)->command_buffer_begin(next);
        },
        [](void *context, uint32_t reopen) {
            auto &scope = *static_cast<GraphScope *>(context);
            scope.change_labels(scope.labels, reopen != 0);
        },
        [](void *context, uint64_t next_id, uint64_t timeline, uint64_t previous_done, uint64_t ready, uint64_t done) {
            auto &scope = *static_cast<GraphScope *>(context);
            auto *vk = static_cast<RenderingDeviceDriverVulkan *>(scope.driver);
            RenderingDeviceDriver::CommandBufferID next;
            next.id = next_id;
            vk->rescaleframe_graph_split(scope.commands, next, timeline, previous_done, ready, done);
            ++scope.pool.buffers_used;
            scope.commands = next;
        },
        [](void *context) -> void * {
            auto &scope = *static_cast<GraphScope *>(context);
            auto *vk = static_cast<RenderingDeviceDriverVulkan *>(scope.driver);
            void *commands = vk->rescaleframe_begin(scope.commands);
            CRASH_COND_MSG(!commands, "ReScaleFrame lost its graph continuation after consuming the source buffer.");
            return commands;
        },
    };
    auto *scope = active && active->driver == user ? active : nullptr;
    return rsf_godot_graph_split(&calls, scope, current_commands, timeline, previous_done, ready, done,
            continuation_commands);
}
}
