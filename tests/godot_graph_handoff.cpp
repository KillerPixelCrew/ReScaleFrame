// SPDX-License-Identifier: MIT
// Compile the real GraphScope against typed driver and graph doubles.
#include "../integrations/godot/modules/rescaleframe/graph.cpp"
#include <cstdio>
#include <optional>

extern "C" void rsf_fixture_vk_labels(void *state, uint64_t command, uint32_t reopen);
extern "C" int rsf_fixture_graph_cases(void *context, void *state, uint32_t reuse, uint32_t refusals);

namespace {
using Driver = RenderingDeviceDriverVulkan;
using Pool = RenderingDeviceGraph::CommandBufferPool;

struct Fixture {
    Driver driver, foreign;
    Driver::CommandBufferID commands{1};
    Pool pool;
    struct Labels {
        Fixture *fixture;
        void *state;
        void operator()(bool reopen) {
            rsf_fixture_vk_labels(state, fixture->commands.id, reopen);
        }
    } labels;
    std::optional<RSFGodot::GraphScope> scope;
    rsf_godot_vk_handoff handoff{};

    Fixture(void *state, void *foreign_state) : driver(state), foreign(foreign_state), labels{this, state} {}
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

extern "C" void rsf_fixture_graph_pool(void *context, uint64_t (*out)[4]) {
    const auto &f = *static_cast<Fixture *>(context);
    (*out)[0] = f.commands.id;
    (*out)[1] = f.pool.buffers_used;
    (*out)[2] = f.pool.buffers.size();
    (*out)[3] = f.pool.semaphores.size();
}

extern "C" void rsf_fixture_graph_failure(uint32_t line) {
    std::printf("godot_graph_handoff.rs:%u: check failed\n", static_cast<unsigned>(line));
}

extern "C" int rsf_fixture_graph_run(void *state, void *foreign_state, uint32_t reuse, uint32_t refusals) {
    Fixture f(state, foreign_state);
    f.pool.pool.id = 9;
    if (reuse) {
        f.pool.buffers.push_back({2});
        f.pool.semaphores.push_back({1});
    }
    return rsf_fixture_graph_cases(&f, state, reuse, refusals);
}

extern "C" int rsf_fixture_graph_main();

int main() {
    return rsf_fixture_graph_main();
}
