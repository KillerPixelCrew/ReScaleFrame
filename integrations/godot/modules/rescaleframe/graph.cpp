// SPDX-License-Identifier: MIT
#include "graph.h"
#include "drivers/vulkan/rendering_device_driver_vulkan.h"

namespace RSFGodot {
struct GraphCalls {
    void *(*current)(void *scope);
    uint64_t (*pooled)(void *scope);
    uint64_t (*allocate)(void *scope);
    uint32_t (*begin)(void *scope, uint64_t next);
    void (*labels)(void *scope, uint32_t reopen);
    void (*consume)(void *scope, uint64_t next, uint64_t timeline, uint64_t previous_done, uint64_t ready, uint64_t done);
    void *(*continuation)(void *scope);
};
static_assert(sizeof(GraphCalls) == 7 * sizeof(void *));
static_assert(VK_ERROR_INITIALIZATION_FAILED == -3 && VK_ERROR_OUT_OF_HOST_MEMORY == -1);
extern "C" int32_t rsf_godot_graph_split(const GraphCalls *calls, void *scope, void *current,
        uint64_t timeline, uint64_t previous_done, uint64_t ready, uint64_t done, void **out);

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
    static constexpr GraphCalls calls{
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
            // The engine frame fence owns every pooled buffer and its binary links.
            auto &pool = scope.pool;
            // Godot can free a semaphore on its own. It retires command buffers with the pool.
            const auto semaphore = vk->semaphore_create();
            if (!semaphore) return 0;
            const auto next = vk->command_buffer_create(pool.pool);
            if (!next) {
                vk->semaphore_free(semaphore);
                return 0;
            }
            pool.buffers.push_back(next);
            pool.semaphores.push_back(semaphore);
            return next.id;
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
