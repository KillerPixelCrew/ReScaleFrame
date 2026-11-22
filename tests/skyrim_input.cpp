// SPDX-License-Identifier: MIT
#include "../games/skyrim-se/src/cpu_events.h"
#include "../games/skyrim-se/src/cpu_policy.h"
#include "../games/skyrim-se/src/input_ownership.h"
#include "../games/skyrim-se/src/screen_classification.h"
#include <windows.h>
#include <array>
#include <cstdio>

namespace {
struct Event { uintptr_t vtable{}; uint32_t device{}, type{}; Event* next{}; };
int failures{};
void check(bool passed, const char* label) {
    if (!passed) { ++failures; std::printf("FAIL %s\n", label); }
}

void check_render_scope() {
    constexpr uintptr_t base = 0x10000000;
    constexpr uintptr_t caller = base + 0x656e32;
    constexpr uint64_t session = 9, source = 17;
    rsf_skyrim_cpu_render_state state{};
    rsf_game_render_pass pass{};
    rsf_game_cpu_event event{};
    rsf_skyrim_cpu_render_receipt outer{}, nested{};
    rsf_skyrim_cpu_frame_event(&event, &state, RSF_GAME_CPU_FRAME_END, session, source, 20, 50);
    check(event.flags == (RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_RENDER_SKIPPED) && event.render_expected == 0,
        "skipped render closes the CPU frame without a completed World");
    check(!rsf_skyrim_cpu_render_identity(&state, &pass, source, true), "identity requires RenderEntry");
    check(rsf_skyrim_cpu_begin_render(&state, RSF_SCREEN_UNKNOWN, &outer), "first RenderEntry closes simulation");
    const auto scope = state.scope;
    check(rsf_skyrim_cpu_render_identity(&state, &pass, source, true) && scope && pass.scope_id == scope &&
        pass.submission_id == scope && pass.source_frame_id == source && (pass.flags & RSF_GAME_RENDER_NO_FG),
        "frame pass is scoped and refuses FG before a primary World");
    int chain{};
    pass.swapchain = &chain;
    pass.view_key = 1;
    check(!rsf_skyrim_cpu_begin_world(&state, &pass, base, caller + 1, 0) &&
        !rsf_skyrim_cpu_begin_world(&state, &pass, base, caller, 1) && state.primary_world_entries == 0,
        "foreign callers and non-primary outputs do not enter the primary World");
    check(rsf_skyrim_cpu_begin_world(&state, &pass, base, caller, 0), "mapped World caller enters the source");
    rsf_skyrim_cpu_world_screen(&state, RSF_SCREEN_GAMEPLAY);
    check(rsf_skyrim_cpu_render_identity(&state, &pass, source, true) && !(pass.flags & RSF_GAME_RENDER_NO_FG) &&
        pass.screen == RSF_SCREEN_GAMEPLAY && pass.scope_id == scope, "gameplay World admits its render scope");
    check(rsf_skyrim_cpu_render_identity(&state, &pass, source, false) && (pass.flags & RSF_GAME_RENDER_NO_FG),
        "inactive observer still labels the scope and refuses FG");
    rsf_skyrim_cpu_end_world(&state);
    check(rsf_skyrim_cpu_begin_world(&state, &pass, base, caller, 0), "duplicate World is observed");
    rsf_skyrim_cpu_world_screen(&state, RSF_SCREEN_GAMEPLAY);
    check(rsf_skyrim_cpu_render_identity(&state, &pass, source, true) && (pass.flags & RSF_GAME_RENDER_NO_FG) &&
        pass.screen == RSF_SCREEN_UNKNOWN, "duplicate World withdraws gameplay admission");
    rsf_skyrim_cpu_end_world(&state);
    check(!rsf_skyrim_cpu_begin_render(&state, RSF_SCREEN_LOADING, &nested) && state.scope != scope,
        "nested RenderEntry starts a distinct scope without repeating simulation end");
    rsf_skyrim_cpu_end_render(&state, &nested);
    check(rsf_skyrim_cpu_render_identity(&state, &pass, source, true) && pass.scope_id == scope &&
        pass.screen == RSF_SCREEN_UNKNOWN, "leaving a nested RenderEntry restores its enclosing scope and screen");
    rsf_skyrim_cpu_end_render(&state, &outer);
    check(!rsf_skyrim_cpu_render_identity(&state, &pass, source, true), "closed render has no live identity");
    rsf_skyrim_cpu_frame_event(&event, &state, RSF_GAME_CPU_FRAME_END, session, source, 30, 50);
    check(event.render_expected == 2 && event.flags == RSF_GAME_CPU_INPUT_KEYED && event.session_id == session &&
        event.source_frame_id == source && event.timestamp_qpc == 30 && event.qpc_frequency == 50,
        "CPU frame end counts returned Worlds and preserves the callback clock");
    check(rsf_skyrim_cpu_sample_tick(&state) && !rsf_skyrim_cpu_sample_tick(&state),
        "tick reports the unreserved native sample once");

    rsf_game_cpu_source_event sample{};
    rsf_skyrim_cpu_source_event(&sample, RSF_GAME_CPU_SOURCE_SAMPLED, session, source, 30, 50,
        0, 8, RSF_GAME_CPU_SOURCE_PRODUCED, RSF_GAME_INPUT_MOUSE, 0, 20);
    check(sample.header.struct_size == sizeof(sample) && sample.header.event_version == RSF_GAME_CPU_EVENT_VERSION_SOURCE &&
        sample.header.stage == RSF_GAME_CPU_SOURCE_SAMPLED && sample.header.source_frame_id == source &&
        sample.header.timestamp_qpc == 30 && sample.poll_begin_qpc == 20 && sample.native_generation == 8 &&
        sample.header.input_kind == RSF_GAME_INPUT_MOUSE && sample.header.flags == RSF_GAME_CPU_SOURCE_PRODUCED,
        "sample receipt preserves both native poll and publication timestamps");
}
}
int main() {
    check_render_scope();
    // The Rust ownership fixtures cover policy transitions. Exercise the native guarded read
    // and C++ calling convention with real addresses in this cross-language fixture.
    using rsf::skyrim::cpu_input_kinds;
    Event event{}; Event* list = &event;
    check(cpu_input_kinds(&list) == RSF_GAME_INPUT_KEYBOARD, "keyboard button");
    event.device = 3; event.type = 2;
    check(cpu_input_kinds(&list) == RSF_GAME_INPUT_KEYBOARD, "virtual keyboard character");
    event.device = 1; event.type = 1;
    check(cpu_input_kinds(&list) == RSF_GAME_INPUT_MOUSE, "mouse movement");
    event.device = 2; event.type = 3;
    check(cpu_input_kinds(&list) == RSF_GAME_INPUT_CONTROLLER, "controller thumbstick");
    event.type = 4;
    check(cpu_input_kinds(&list) == 0, "device connection is not delivered control input");
    event.device = 99; event.type = 0;
    check(cpu_input_kinds(&list) == 0, "unknown device");
    event.device = 0; event.type = 1;
    check(cpu_input_kinds(&list) == 0, "mismatched device and movement type");
    Event mouse{0, 1, 0, nullptr}, controller{0, 2, 0, nullptr};
    event.type = 0; event.next = &mouse; mouse.next = &controller;
    check(cpu_input_kinds(&list) == 7, "mixed list aggregates delivered kinds");
    controller.next = &event;
    check(cpu_input_kinds(&list) == 0, "cyclic list refuses partial activity");
    event.next = reinterpret_cast<Event*>(1);
    check(cpu_input_kinds(&list) == 0, "unreadable node refuses partial activity");
    check(cpu_input_kinds(reinterpret_cast<const void*>(1)) == 0, "unreadable head pointer");
    check(cpu_input_kinds(nullptr) == 0, "absent head pointer");
    list = nullptr;
    check(cpu_input_kinds(&list) == 0, "empty list");
    std::array<Event, 129> long_list{};
    for (size_t i = 0; i + 1 < long_list.size(); ++i) long_list[i].next = &long_list[i + 1];
    list = long_list.data();
    check(cpu_input_kinds(&list) == 0, "excessive list refuses partial activity");
    long_list[127].next = nullptr;
    check(cpu_input_kinds(&list) == RSF_GAME_INPUT_KEYBOARD, "bounded list terminates successfully");
    return failures ? 1 : 0;
}
