// SPDX-License-Identifier: MIT
// Compile the real GraphScope against typed driver and graph doubles.
#include "../integrations/godot/modules/rescaleframe/graph.cpp"
#include <cstdio>
#include <optional>

namespace {
using Driver = RenderingDeviceDriverVulkan;
using Pool = RenderingDeviceGraph::CommandBufferPool;

struct Fixture {
    Driver driver, foreign;
    Driver::CommandBufferID commands{1};
    Pool pool;
    struct Labels {
        Fixture *fixture;
        void operator()(bool reopen) {
            auto &f = *fixture;
            if (reopen) {
                ++f.opens;
                f.labels_valid &= f.commands.id == 2 && f.driver.state[1].ended;
            } else {
                ++f.closes;
                f.labels_valid &= f.commands.id == 1 && !f.driver.state[1].ended && f.driver.state[2].begun;
            }
        }
    } labels{this};
    uint32_t closes = 0, opens = 0;
    bool labels_valid = true;
    std::optional<RSFGodot::GraphScope> scope;
    rsf_godot_vk_handoff handoff{};
};
}

extern "C" void rsf_fixture_graph_scope(void *context, uint32_t active) {
    auto &f = *static_cast<Fixture *>(context);
    if (active) f.scope.emplace(&f.driver, f.commands, f.pool, f.labels);
    else f.scope.reset();
}

extern "C" uint32_t rsf_fixture_graph_handoff(void *context, uint32_t foreign, uint64_t command_override) {
    auto &f = *static_cast<Fixture *>(context);
    auto *driver = foreign ? &f.foreign : &f.driver;
    auto commands = command_override ? Driver::CommandBufferID{command_override} : f.commands;
    return RSFGodot::GraphScope::handoff(driver, commands, f.handoff);
}

extern "C" uint32_t rsf_fixture_graph_header(void *context) {
    const auto &f = *static_cast<Fixture *>(context);
    return f.handoff.user == &f.driver && f.handoff.queue_family == 3 &&
            f.handoff.timeline_enabled == 1 && f.handoff.struct_size == sizeof(f.handoff) &&
            f.handoff.abi_version == RSF_GODOT_VK_HANDOFF_ABI_VERSION;
}

extern "C" int32_t rsf_fixture_graph_split(void *context, uint32_t foreign, size_t command_override,
        uint64_t timeline, uint64_t previous, uint64_t ready, uint64_t done, size_t *output) {
    auto &f = *static_cast<Fixture *>(context);
    if (!f.handoff.split_record) return VK_ERROR_INITIALIZATION_FAILED;
    auto *driver = foreign ? &f.foreign : &f.driver;
    auto *commands = command_override ? reinterpret_cast<void *>(command_override) : f.driver.rescaleframe_begin(f.commands);
    void *next = output ? reinterpret_cast<void *>(*output) : nullptr;
    const auto result = f.handoff.split_record(driver, commands, timeline, previous, ready, done, output ? &next : nullptr);
    if (output) *output = reinterpret_cast<size_t>(next);
    return result;
}

extern "C" void rsf_fixture_graph_fault(void *context, uint32_t fault) {
    auto &vk = static_cast<Fixture *>(context)->driver;
    vk.timeline_enabled = fault != 1;
    vk.fail_allocate = fault == 2;
    vk.fail_semaphore = fault == 3;
    vk.fail_begin = fault == 4;
}

extern "C" void rsf_fixture_graph_snapshot(void *context, uint64_t *out) {
    const auto &f = *static_cast<Fixture *>(context);
    const auto &vk = f.driver;
    const uint64_t values[]{
        f.commands.id, f.pool.buffers_used, f.pool.buffers.size(), f.pool.semaphores.size(),
        vk.allocated, vk.begins, vk.splits, vk.freed_semaphores,
        vk.state[1].timeline, vk.state[1].wait, vk.state[1].signal,
        vk.state[2].timeline, vk.state[2].wait, vk.state[2].signal,
        f.closes, f.opens, f.labels_valid,
    };
    for (size_t i = 0; i < sizeof(values) / sizeof(values[0]); ++i) out[i] = values[i];
}

extern "C" int rsf_fixture_graph_cases(void *context, uint32_t reuse, uint32_t refusals);

extern "C" void rsf_fixture_graph_failure(uint32_t line) {
    std::printf("godot_graph_handoff.rs:%u: check failed\n", static_cast<unsigned>(line));
}

int main() {
    int failures = 0;
    for (uint32_t scenario = 0; scenario < 3; ++scenario) {
        Fixture f;
        f.pool.pool.id = 9;
        if (scenario == 1) {
            f.pool.buffers.push_back({2});
            f.pool.semaphores.push_back({1});
            f.driver.allocated = 2;
        }
        failures += rsf_fixture_graph_cases(&f, scenario == 1, scenario == 2);
    }
    return failures ? 1 : 0;
}
