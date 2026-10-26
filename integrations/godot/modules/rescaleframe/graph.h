// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include "api.h"
#include "servers/rendering/rendering_device_graph.h"

namespace RSFGodot {
// This lease exists only while a graph callback owns the current buffer reference.
// The native provider retains the driver identity, never this stack object.
class GraphScope {
    RenderingDeviceDriver *driver;
    RenderingDeviceDriver::CommandBufferID &commands;
    RenderingDeviceGraph::CommandBufferPool &pool;
    void *labels;
    void (*change_labels)(void *, bool);
    GraphScope *previous;
    static thread_local GraphScope *active;
    static int32_t split_record(void *user, void *current_commands, uint64_t timeline,
            uint64_t previous_done, uint64_t ready, uint64_t done, void **continuation_commands);

public:
    template <class Labels>
    GraphScope(RenderingDeviceDriver *p_driver, RenderingDeviceDriver::CommandBufferID &p_commands,
            RenderingDeviceGraph::CommandBufferPool &p_pool, Labels &p_labels) :
            driver(p_driver), commands(p_commands), pool(p_pool), labels(&p_labels),
            change_labels([](void *user, bool reopen) { (*static_cast<Labels *>(user))(reopen); }), previous(active) {
        active = this;
    }
    ~GraphScope() { active = previous; }
    GraphScope(const GraphScope &) = delete;
    GraphScope &operator=(const GraphScope &) = delete;

    static bool handoff(RenderingDeviceDriver *driver, RenderingDeviceDriver::CommandBufferID commands,
            rsf_godot_vk_handoff &out);
};
}
