// SPDX-License-Identifier: MIT
#include "native_fg_cpu_platform.h"
#include <rescaleframe/game_api.h>
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <future>
#include <mutex>
#include <thread>
#include <utility>
#include <vector>

namespace {
using namespace std::chrono_literals;
constexpr uint64_t session = 41;
constexpr uint64_t primary_window = 51;
constexpr uint64_t replacement_window = 52;
std::atomic<int> failures{};
#define CHECK(x) do { if (!(x)) { std::printf("FAIL %d: %s\n", __LINE__, #x); ++failures; } } while (false)

rsf_nfg_cpu_platform platform{};
std::atomic<uint64_t> generation{11};
std::atomic<uint32_t> pace_calls{}, abort_calls{}, receipt_calls{};
std::mutex guard;
std::condition_variable changed;
uint64_t blocked_source{};
bool sleeping{}, may_wake{}, waiting_for_predecessor{};
std::vector<std::pair<uint64_t, uint32_t>> markers;
bool rebind_during_receipt{};

uint64_t stamp() {
    LARGE_INTEGER value{};
    QueryPerformanceCounter(&value);
    return uint64_t(value.QuadPart);
}

rsf_game_cpu_event fact(uint64_t source, uint32_t stage) {
    rsf_game_cpu_event event{};
    event.struct_size = sizeof(event);
    event.stage = stage;
    event.session_id = session;
    event.source_frame_id = source;
    event.timestamp_qpc = stamp();
    LARGE_INTEGER frequency{};
    QueryPerformanceFrequency(&frequency);
    event.qpc_frequency = uint64_t(frequency.QuadPart);
    event.event_version = RSF_GAME_CPU_EVENT_VERSION;
    event.flags = RSF_GAME_CPU_INPUT_KEYED;
    return event;
}

int send(const rsf_game_cpu_event& event) {
    return rsf_native_cpu_dispatch(&event, +[](void*, const rsf_game_cpu_event* e) {
        rsf_core_native_fg_cpu(e, &platform);
    }, nullptr);
}

int source_event(uint64_t source, uint32_t stage, uint64_t predecessor = 0) {
    rsf_game_cpu_source_event event{};
    event.header = fact(source, stage);
    event.header.struct_size = sizeof(event);
    event.header.event_version = RSF_GAME_CPU_EVENT_VERSION_SOURCE;
    event.header.flags = 0;
    event.native_generation = 1;
    event.predecessor_source_id = predecessor;
    if (predecessor != 0) event.header.flags = RSF_GAME_CPU_SOURCE_OVERLAPS_PREDECESSOR;
    if (stage == RSF_GAME_CPU_SOURCE_ABORT) event.reason = RSF_GAME_CPU_ABORT_QUIESCE;
    return send(event.header);
}

int bind(uint64_t window) {
    const rsf_native_latency_window binding{sizeof(binding), 1, session, window};
    return rsf_core_native_latency_window_bind(&binding, &platform);
}

int receipt(uint64_t id, uint64_t window) {
    rsf_game_cpu_receipt_event event{};
    event.header = fact(0, RSF_GAME_CPU_INPUT_EVENT);
    event.header.struct_size = sizeof(event);
    event.header.event_version = RSF_GAME_CPU_EVENT_VERSION_RECEIPT;
    event.header.flags = RSF_GAME_CPU_INPUT_UNFRAMED;
    event.header.input_kind = RSF_GAME_INPUT_MOUSE;
    event.receipt_id = id;
    event.window_key = window;
    return send(event.header);
}

void arm(uint64_t id) {
    std::lock_guard lock(guard);
    sleeping = may_wake = false;
    blocked_source = id;
}

bool wait_for_sleep() {
    std::unique_lock lock(guard);
    return changed.wait_for(lock, 2s, [] { return sleeping; });
}

void wake() {
    {
        std::lock_guard lock(guard);
        may_wake = true;
    }
    changed.notify_all();
}

rsf_native_latency_frame snapshot(uint64_t source) {
    rsf_native_latency_frame out{sizeof(out), 1, source};
    CHECK(rsf_core_native_latency_frame_get(&out));
    return out;
}

rsf_native_input_routes routes() {
    rsf_native_input_routes out{sizeof(out), 1};
    CHECK(rsf_core_native_input_routes_get(&out));
    return out;
}

void mock_platform() {
    platform = rsf_native_fg_cpu_platform;
    platform.has_owner = +[] { return 1; };
    platform.generation = +[] { return generation.load(); };
    platform.backend = +[] { return RSF_FG_BACKEND_XESS; };
    platform.device = +[]() -> void* { return &platform; };
    platform.adapter_vendor = +[](void*) { return 0x8086u; };
    platform.host = +[]() -> void* { return nullptr; };
    platform.status = +[](rsf_d3d11_present_status* out) {
        out->backend = RSF_FG_BACKEND_XESS;
        out->state.pacing_owner = RSF_PACING_XELL;
        out->state.low_latency_available = 1;
        return RSF_BACKEND_OK;
    };
    platform.acquire = +[](uint64_t) { return RSF_BACKEND_OK; };
    platform.pace = +[](uint64_t source, uint32_t owner, uint64_t expected) {
        CHECK(owner == RSF_PACING_XELL);
        CHECK(expected == generation.load());
        ++pace_calls;
        std::unique_lock lock(guard);
        if (source == blocked_source) {
            sleeping = true;
            changed.notify_all();
            if (!changed.wait_for(lock, 3s, [] { return may_wake; })) return RSF_BACKEND_ERROR_NOT_READY;
        }
        return RSF_BACKEND_OK;
    };
    platform.marker = +[](uint64_t expected, uint64_t source, uint32_t marker, uint32_t controller) {
        CHECK(expected == generation.load() && controller == 0);
        {
            std::lock_guard lock(guard);
            markers.emplace_back(source, marker);
        }
        if (source == 1 && marker == RSF_LATENCY_PRESENT_END) wake();
        return RSF_BACKEND_OK;
    };
    platform.receipt = +[](uint64_t expected, uint64_t source, uint32_t kind, uint32_t message) {
        CHECK(expected == generation.load() && source == 2 && kind == RSF_GAME_INPUT_MOUSE && message == 0);
        ++receipt_calls;
        if (rebind_during_receipt) CHECK(bind(replacement_window) == RSF_OK);
        return RSF_BACKEND_OK;
    };
    platform.abort = +[](uint64_t) { ++abort_calls; return RSF_BACKEND_OK; };
    platform.wait_completion = +[](uint32_t timeout_ms, uint32_t pump_sent_messages) {
        if (!pump_sent_messages) {
            {
                std::lock_guard lock(guard);
                waiting_for_predecessor = true;
            }
            changed.notify_all();
        }
        return rsf_native_fg_cpu_platform.wait_completion(timeout_ms, pump_sent_messages);
    };
    platform.owned_window_thread = +[](uint64_t key) -> uint32_t {
        return key == primary_window || key == replacement_window ? GetCurrentThreadId() : 0;
    };
}
void sample_native_thread() {
    const auto& native = rsf_native_fg_cpu_platform;
    std::promise<void*> duplicated;
    auto handle = duplicated.get_future();
    std::atomic<bool> done{};
    std::thread renderer([&] {
        duplicated.set_value(native.duplicate_render_thread());
        while (!done.load()) Sleep(1);
    });
    void* thread = handle.get();
    CHECK(thread != nullptr);
    if (thread) {
        uint64_t addresses[9]{};
        const size_t count = native.sample_render_thread(thread, addresses, 9);
        CHECK(count > 0 && count <= 9);
        CHECK(addresses[0] != 0);
        char location[384]{};
        CHECK(native.describe_address(addresses[0], location, sizeof(location)) > 0);
    }
    done.store(true);
    renderer.join();
    if (thread) CloseHandle(thread);
}
} // namespace

int main() {
    sample_native_thread();
    mock_platform();
    rsf_core_native_fg_reflex_ordering_set(1);
    CHECK(send(fact(1, RSF_GAME_CPU_FRAME_BEGIN)));
    CHECK(send(fact(1, RSF_GAME_CPU_INPUT_SAMPLE)));
    CHECK(send(fact(1, RSF_GAME_CPU_SIMULATION_BEGIN)));
    CHECK(send(fact(1, RSF_GAME_CPU_SIMULATION_END)));
    rsf_core_native_latency_render(1, 1, &platform);
    CHECK(send(fact(1, RSF_GAME_CPU_FRAME_END)));

    CHECK(send(fact(2, RSF_GAME_CPU_FRAME_BEGIN)));
    arm(2);
    std::thread next([] { CHECK(send(fact(2, RSF_GAME_CPU_INPUT_SAMPLE))); });
    CHECK(wait_for_sleep());
    CHECK(!snapshot(2).paced);
    const auto duplicate = fact(2, RSF_GAME_CPU_INPUT_SAMPLE);
    rsf_core_native_fg_cpu(&duplicate, &platform);
    CHECK(pace_calls == 2);
    rsf_core_native_latency_render(1, 0, &platform);
    rsf_core_native_latency_present(1, nullptr, 1, &platform);
    rsf_core_native_latency_present(1, nullptr, 0, &platform);
    next.join();
    CHECK(snapshot(2).paced && snapshot(2).generation == generation.load() && snapshot(1).completed);
    for (uint32_t marker = 0; marker < 6; ++marker) {
        CHECK(std::ranges::count(markers, std::pair<uint64_t, uint32_t>{1, marker}) == 1);
    }

    CHECK(bind(primary_window) == RSF_OK);
    CHECK(send(fact(2, RSF_GAME_CPU_SIMULATION_BEGIN)));
    CHECK(receipt(1, 0) && receipt_calls == 1 && routes().matched_thread == 1);
    CHECK(!receipt(1, 0) && receipt_calls == 1);
    CHECK(receipt(2, replacement_window) && receipt_calls == 1 && routes().unmatched == 1);
    CHECK(receipt(3, primary_window) && receipt_calls == 2 && routes().matched_window == 1);
    CHECK(routes().last_receipt == 3 && routes().last_source == 2 && routes().routed == 2);
    rebind_during_receipt = true;
    CHECK(receipt(4, primary_window) && receipt_calls == 3);
    CHECK(routes().last_source == 0 && routes().routed == 0);

    // A source admitted by the old provider cannot mark the new provider.
    const auto before_switch = markers.size();
    ++generation;
    CHECK(send(fact(2, RSF_GAME_CPU_SIMULATION_END)));
    CHECK(markers.size() == before_switch);
    CHECK(send(fact(2, RSF_GAME_CPU_FRAME_END)));

    arm(3);
    std::thread reserved([] { CHECK(source_event(3, RSF_GAME_CPU_SOURCE_RESERVE, 2)); });
    {
        std::unique_lock lock(guard);
        CHECK(changed.wait_for(lock, 2s, [] { return waiting_for_predecessor; }));
        CHECK(!sleeping && pace_calls == 2);
    }
    CHECK(rsf_native_cpu_present_result(session, 2, RSF_NATIVE_CPU_PRESENT_RETURNED, 0));
    CHECK(wait_for_sleep());
    CHECK(!snapshot(3).paced && snapshot(3).ready_qpc == 0);
    const auto aborts = abort_calls.load();
    CHECK(source_event(3, RSF_GAME_CPU_SOURCE_ABORT));
    CHECK(snapshot(3).failed && abort_calls == aborts);
    wake();
    reserved.join();
    CHECK(!snapshot(3).paced && abort_calls == aborts + 1);

    CHECK(send(fact(4, RSF_GAME_CPU_FRAME_BEGIN)));
    const auto admitted_generation = generation.fetch_add(1);
    CHECK(send(fact(4, RSF_GAME_CPU_INPUT_SAMPLE)));
    CHECK(pace_calls == 3 && snapshot(4).failed && snapshot(4).generation == admitted_generation);
    CHECK(send(fact(4, RSF_GAME_CPU_FRAME_END)));

    rsf_core_native_latency_end_session(session + 1, &platform);
    CHECK(snapshot(3).failed && rsf_native_latency_has_claim(1));
    rsf_core_native_latency_end_session(session, &platform);
    CHECK(!rsf_native_latency_has_claim(1));
    rsf_native_latency_frame retired{sizeof(retired), 1, 3};
    CHECK(!rsf_core_native_latency_frame_get(&retired));

    rsf_native_fg_rendered_source_set(1);
    rsf_core_native_fg_reflex_ordering_set(0);
    const auto previous_markers = markers.size();
    CHECK(send(fact(1, RSF_GAME_CPU_FRAME_BEGIN)));
    CHECK(!send(fact(1, RSF_GAME_CPU_INPUT_SAMPLE)));
    CHECK(send(fact(1, RSF_GAME_CPU_SIMULATION_END)));
    rsf_game_render_pass pass{};
    pass.struct_size = sizeof(pass);
    pass.role = RSF_GAME_RENDER_FRAME;
    pass.session_id = session;
    pass.source_frame_id = 1;
    pass.native_frame = 1;
    rsf_core_native_fg_frame(&pass, 1, &platform);
    rsf_core_native_latency_render(1, 1, &platform);
    rsf_core_native_latency_present(1, nullptr, 1, &platform);
    rsf_core_native_latency_present(1, nullptr, 0, &platform);
    rsf_core_native_fg_frame(&pass, 0, &platform);
    CHECK(send(fact(1, RSF_GAME_CPU_FRAME_END)));
    CHECK(!snapshot(1).paced && !snapshot(1).failed && snapshot(1).completed);
    CHECK(pace_calls == 3 && markers.size() == previous_markers && !rsf_native_latency_has_claim(1));
    rsf_core_native_latency_end_session(session, &platform);
    return failures.load() != 0;
}
