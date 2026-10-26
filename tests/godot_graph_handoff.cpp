// SPDX-License-Identifier: GPL-3.0-only
// Compile the actual graph lease with a narrow engine seam. No GPU or queue calls.
#include "../integrations/godot/modules/rescaleframe/graph.cpp"
#include <cstdio>

namespace {
int failed = 0;
#define CHECK(expression) do { if (!(expression)) { std::printf("line %d: %s\n", __LINE__, #expression); ++failed; } } while (false)
using Driver = RenderingDeviceDriverVulkan;
using Pool = RenderingDeviceGraph::CommandBufferPool;
void exercise(bool reuse) {
    Driver vk;
    Driver::CommandBufferID commands{1};
    Pool pool; pool.pool.id = 9;
    if (reuse) { pool.buffers.push_back({2}); pool.semaphores.push_back({1}); vk.allocated = 2; }
    const auto initial = commands;
    uint32_t closes = 0, opens = 0;
    auto labels = [&](bool reopen) {
        if (reopen) { ++opens; CHECK(commands.id == 2); CHECK(vk.state[1].ended); }
        else { ++closes; CHECK(commands == initial); CHECK(!vk.state[1].ended); CHECK(vk.state[2].begun); }
    };
    rsf_godot_vk_handoff handoff{};
    CHECK(!RSFGodot::GraphScope::handoff(&vk, commands, handoff));
    {
        RSFGodot::GraphScope scope(&vk, commands, pool, labels);
        CHECK(RSFGodot::GraphScope::handoff(&vk, commands, handoff));
        CHECK(handoff.user == &vk && handoff.queue_family == 3 && handoff.timeline_enabled == 1);
        CHECK(handoff.struct_size == sizeof(handoff) && handoff.abi_version == RSF_GODOT_VK_HANDOFF_ABI_VERSION);
        void *continuation = nullptr;
        const auto split = handoff.split_record;
        CHECK(split(&vk, vk.rescaleframe_begin(commands), 77, 4, 5, 6, &continuation) == 0);
        CHECK(commands.id == 2 && continuation == vk.rescaleframe_begin(commands));
        CHECK(pool.buffers_used == 1 && pool.buffers.size() == 1 && pool.semaphores.size() == 1);
        CHECK(vk.allocated == 2 && vk.begins == 1 && vk.splits == 1);
        CHECK(vk.state[1].timeline == 77 && vk.state[1].wait == 4 && vk.state[1].signal == 5);
        CHECK(vk.state[2].timeline == 77 && vk.state[2].wait == 6 && vk.state[2].signal == 0);
        CHECK(closes == 1 && opens == 1);
        // Native SDK failure after the handoff does not undo engine ownership.
        CHECK(!RSFGodot::GraphScope::handoff(&vk, commands, handoff));
    }
    void *continuation = reinterpret_cast<void *>(1);
    CHECK(handoff.split_record(&vk, vk.rescaleframe_begin(commands), 77, 6, 7, 8, &continuation) != 0);
    CHECK(!continuation && commands.id == 2 && pool.buffers_used == 1);
}
void refusals() {
    Driver vk, foreign;
    Driver::CommandBufferID commands{1};
    Pool pool; pool.pool.id = 9;
    uint32_t labels_changed = 0;
    auto labels = [&](bool) { ++labels_changed; };
    RSFGodot::GraphScope scope(&vk, commands, pool, labels);
    rsf_godot_vk_handoff handoff{};
    CHECK(!RSFGodot::GraphScope::handoff(&foreign, commands, handoff));
    CHECK(!RSFGodot::GraphScope::handoff(&vk, {2}, handoff));
    vk.timeline_enabled = false;
    CHECK(!RSFGodot::GraphScope::handoff(&vk, commands, handoff));
    vk.timeline_enabled = true;
    CHECK(RSFGodot::GraphScope::handoff(&vk, commands, handoff));
    void *next = nullptr;
    const auto split = handoff.split_record;
    CHECK(split(&foreign, vk.rescaleframe_begin(commands), 77, 4, 5, 6, &next) != 0);
    CHECK(split(&vk, reinterpret_cast<void *>(17), 77, 4, 5, 6, &next) != 0);
    CHECK(split(&vk, vk.rescaleframe_begin(commands), 0, 4, 5, 6, &next) != 0);
    CHECK(split(&vk, vk.rescaleframe_begin(commands), 77, 5, 5, 6, &next) != 0);
    CHECK(split(&vk, vk.rescaleframe_begin(commands), 77, 4, 5, 5, &next) != 0);
    CHECK(split(&vk, vk.rescaleframe_begin(commands), 77, 4, 5, 6, nullptr) != 0);
    vk.fail_allocate = true;
    CHECK(split(&vk, vk.rescaleframe_begin(commands), 77, 4, 5, 6, &next) != 0);
    vk.fail_allocate = false; vk.fail_semaphore = true;
    CHECK(split(&vk, vk.rescaleframe_begin(commands), 77, 4, 5, 6, &next) != 0);
    vk.fail_semaphore = false; vk.fail_begin = true;
    CHECK(split(&vk, vk.rescaleframe_begin(commands), 77, 4, 5, 6, &next) != 0);
    CHECK(!next && commands.id == 1 && pool.buffers_used == 0 && vk.splits == 0 && !labels_changed);
}
}
int main() {
    exercise(false); exercise(true); refusals();
    if (!failed) std::puts("Godot graph recording handoff passed (synthetic, no GPU).");
    return failed ? 1 : 0;
}
